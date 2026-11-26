# Tests exercise host-side range validation and recovery using a NOR model.
source [file join [file dirname [info script]] flash_trial.tcl]

proc check {condition} {
    if {![uplevel 1 [list expr $condition]]} { error "assertion failed: $condition" }
}
proc halt {timeout} {
    set ::core_state halted
    set ::polled_state halted
}
proc ready {} { return 0 }

check {[catch {require_sector 0x1000 [lrepeat 1024 0]}]}
check {[catch {require_sector 0x2000 [lrepeat 1023 0]}]}
check {![catch {require_sector 0x2000 [lrepeat 1024 0]}]}
check {[catch {require_page 0x20ff}]}
check {[catch {require_page 0x1000}]}
check {![catch {require_page 0x2f00}]}
check {![catch {require_page 0}]}
check {![catch {verify_diagnostic {0x39325246 1 1 0x410fd210 0x10002000 0x00890087 0x00101002 0}}]}
check {![catch {verify_diagnostic {0x39325246 1 1 0x410fd210 0x10002000 0x80890087 0x00101002 0}}]}
check {[catch {verify_diagnostic {0x39325246 1 1 0x410fd210 0x10002000 0x00890007 0x00101002 0}}]}

rename burn_sector production_burn_sector
proc burn_sector {offset words} {
    lappend ::attempts [list $offset [lindex $words 0]]
    if {$::inject_error && [lindex $words 0] == 0xcafe && $offset == 0} {
        set ::inject_error 0
        error "candidate failed"
    }
}
set stock [dict create 0x2000 [lrepeat 1024 0x1234] 0x0 [lrepeat 1024 0x1234]]
set candidate [dict create 0x2000 [lrepeat 1024 0xcafe] 0x0 [lrepeat 1024 0xcafe]]
# Real full-size files exercise the production digest and sector decoder.
proc model_backup {variant} {
    return [exec python3 -c {
import hashlib, os, struct, sys, tempfile
data = bytearray(1048576)
for offset in (0, 8192):
    struct.pack_into("<1024I", data, offset, *([0x1234] * 1024))
if sys.argv[1] == "other":
    struct.pack_into("<I", data, 8192 + 40, 0x0123)
fd, path = tempfile.mkstemp()
with os.fdopen(fd, "wb") as f:
    f.write(data)
print("{" + path + "}", *struct.unpack("<8I", hashlib.sha256(data).digest()))
} $variant]
}
set fixture [model_backup stock]
dict set stock backup_file [lindex $fixture 0]
dict set stock backup_digest [lrange $fixture 1 end]
dict set candidate 0x0 [lreplace [dict get $candidate 0x0] 55 63 0x3130464d {*}[dict get $stock backup_digest]]
proc verify_image {file address format} {
    check {$file eq [dict get $::stock backup_file] && $address == 0x10000000 && $format eq "bin"}
}
set attempts {}
set inject_error 0
write_pair $candidate
check {$attempts eq {{0x2000 0xcafe} {0 0xcafe}}}

set attempts {}
set inject_error 1
set rollback_needed 0
check {[catch {install_pair $stock $candidate} message]}
check {$attempts eq {{0x2000 0xcafe} {0 0xcafe} {0x2000 0x1234} {0 0x1234}}}
check {!$rollback_needed}
check {[string match {*candidate failed*} $message]}
puts "Flash ranges, application-before-header order and rollback passed."

# Exercise the transaction and burn_sector. Only the board I/O is simulated.
rename burn_sector {}
rename production_burn_sector burn_sector
proc echo {message} {}
proc sleep {ms} {}
proc poll {} {
    if {$::fail_poll} { error "target poll failed" }
    set ::polled_state $::core_state
}
proc target {command} {
    check {$command eq "current"}
    return mock_cpu
}
proc mock_cpu {command} {
    check {$command eq "curstate"}
    return $::polled_state
}
proc resume {} {
    if {$::fail_resume} { error "resume failed" }
    set ::core_state running
}
proc wait_word {args} {}
proc get_reg {args} {
    if {$::xpsr eq "unreadable"} { error "xPSR read failed" }
    return [dict create xPSR $::xpsr]
}
proc status1 {} { return [expr {$::sr1 | $::wel}] }
proc status2 {} { return 2 }
proc ready {} { return [status1] }
proc set_volatile_protection {value} {
    lappend ::protections $value
    if {$value == 0x7c && $::fail_reprotect} { error "protection readback failed" }
    set ::sr1 $value
}
proc reset {action} {
    incr ::resets
    check {$::sr1 == 0x7c && !$::wel}
}
proc put_word {address value} {
    if {$address == 0xe00a4094} { set ::erase_offset [format %d $value] }
}
proc stig {command} {
    switch -- $command {
        0x9fa00000 {}
        0x04000000 { set ::wel 0 }
        0x06000000 {
            check {$::sr1 == 0}
            set ::wel 2
        }
        0x200a0000 {
            check {$::wel == 2}
            set ::wel 0
            set ::sectors($::erase_offset) [lrepeat 1024 0xffffffff]
            lappend ::erases $::erase_offset
        }
        default { error "unexpected controller command $command" }
    }
}
proc word {address} {
    switch -- $address {
        0xe000ed00 { return 0x410fd210 }
        0xe0060000 { return $::system_watchdog }
        0xe0080008 { return $::pmu_watchdog }
        0xe00a40a0 { return 0x146085 }
        default { error "unexpected register $address" }
    }
}
proc read_sector {offset} { return $::sectors([format %d $offset]) }
proc write_memory {address width data} {
    if {$address == 0x20006f40} { set ::witness $data; return }
    check {$address == 0x20003000}
    set ::helper_ram $data
}
proc read_memory {address width count} {
    switch -- $address {
        0xe00a4000 { return {0x80890087 0x0802006b 2 0x08000404 5 0x00101002 0 0 0 0} }
        0xe00a4050 { return {0 0 0} }
        0x20003000 { return $::helper_ram }
        0x20006f40 { return $::witness }
        0xe000ed08 { return {0x10002000} }
        0x10002000 { return [lrange $::sectors(8192) 0 1] }
        default { error "unexpected memory read $address" }
    }
}
proc program_page {offset data} {
    set sector [expr {$offset & ~0xfff}]
    set index [expr {($offset & 0xfff) / 4}]
    set end [expr {$index + 63}]
    check {$::sr1 == 0 && !$::wel}
    check {[lrange $::sectors($sector) $index $end] eq [lrepeat 64 0xffffffff]}
    if {$offset == $::fail_page} { error "page writer interrupted" }
    set ::sectors($sector) [lreplace $::sectors($sector) $index $end {*}$data]
}
set helper {1 2 3 4}
set ota [lrepeat 1024 0]
proc start_trial {app header} {
    set ::sectors(0) $header
    set ::sectors(4096) $::ota
    set ::sectors(8192) $app
    set ::sr1 0x7c
    set ::wel 0
    set ::protections {}
    set ::erases {}
    set ::resets 0
    set ::fail_page -1
    set ::fail_reprotect 0
    set ::core_state running
    set ::witness [linsert [dict get $::stock backup_digest] 0 0x3130464d]
    set ::polled_state unknown
    set ::fail_poll 0
    set ::fail_resume 0
    set ::xpsr 0
    set ::system_watchdog 0
    set ::pmu_watchdog 0
}
proc trial {action} { flash_trial $action $::stock $::candidate $::ota $::helper }

set stock_app [dict get $stock 0x2000]
set stock_header [dict get $stock 0x0]
set probe_app [dict get $candidate 0x2000]
set probe_header [dict get $candidate 0x0]

# Only a known-running install may resume after a read-only rejection.
foreach {action initial expected} {
    install running running
    install halted halted
    install unknown halted
    install debug-running halted
    restore running halted
} {
    start_trial $stock_app $stock_header
    set core_state $initial
    set xpsr 16
    check {[catch {trial $action} message] && [string match {*exception*} $message]}
    check {$core_state eq $expected && $resets == 0 && $protections eq {} && $erases eq {}}
}
start_trial $stock_app $stock_header
set system_watchdog 1
check {[catch {trial install} message] && [string match {*system watchdog*} $message]}
check {$core_state eq "running" && $resets == 0}

start_trial $stock_app $stock_header
set polled_state running
set fail_poll 1
set xpsr 16
check {[catch {trial install} message] && [string match {*exception*} $message]}
check {$core_state eq "halted" && $resets == 0}

start_trial $stock_app $stock_header
set fail_resume 1
set xpsr 16
check {[catch {trial install} message] && [string match {*exception*resume failed*} $message]}
check {$core_state eq "halted" && $resets == 0}

# An unreadable CPU or a failure after programming the PMU read command leaves it stopped.
foreach {xpsr_value pmu message_pattern} {
    unreadable 0 {*xPSR read failed*}
    0 1 {*PMU watchdog*}
} {
    start_trial $stock_app $stock_header
    set xpsr $xpsr_value
    set pmu_watchdog $pmu
    check {[catch {trial install} message] && [string match $message_pattern $message]}
    check {$core_state eq "halted" && $resets == 0}
}

# Fail the third header page after the original app and two pages have been written.
start_trial $probe_app $probe_header
set fail_page 0x200
check {[catch {trial restore} message] && [string match {*page writer interrupted*} $message]}
check {[same_words $sectors(8192) $stock_app]}
check {[lindex $sectors(0) 127] == 0x1234 && [lindex $sectors(0) 128] == 0xffffffff}
check {$sr1 == 0x7c && $resets == 0}
set erases {}
set fail_page -1
trial restore
check {$erases eq {0} && [same_words $sectors(0) $stock_header] && [same_words $sectors(4096) $ota]}
check {$resets == 1 && $sr1 == 0x7c}

# Retry an app write after erase or after one page. A previous install may
# also have stopped before the original header was replaced.
foreach {failure header expected_erases} [list 0x2000 $probe_header {8192 0} \
        0x2100 $probe_header {8192 0} 0x2100 $stock_header {8192}] {
    start_trial $probe_app $header
    set fail_page $failure
    check {[catch {trial restore} message] && [string match {*page writer interrupted*} $message]}
    check {$resets == 0 && $sr1 == 0x7c}
    set fail_page -1
    set erases {}
    trial restore
    check {$erases eq $expected_erases && [same_words $sectors(8192) $stock_app] && [same_words $sectors(0) $stock_header]}
}
foreach {app header expected_erases} [list $stock_app $probe_header {0} \
        $probe_app $stock_header {8192} $stock_app $stock_header {}] {
    start_trial $app $header
    trial restore
    check {$erases eq $expected_erases && $resets == 1}
    if {$expected_erases eq {}} { check {$protections eq {}} }
}

# Invalid inputs must fail preflight with both sectors still protected.
set unknown [lreplace $stock_app 100 100 0xdead]
set partial [concat [lrepeat 64 0x1234] [lrepeat 960 0xffffffff]]
set out_of_order [lreplace $partial 192 255 {*}[lrepeat 64 0x1234]]
foreach {action app header metadata} [list \
        restore $unknown $probe_header $ota \
        restore $stock_app $unknown $ota \
        restore $partial $partial $ota \
        restore $probe_app $partial $ota \
        restore $stock_app $out_of_order $ota \
        restore $probe_app $probe_header $unknown \
        install $stock_app $probe_header $ota \
        install $stock_app $partial $ota] {
    start_trial $app $header
    set sectors(4096) $metadata
    check {[catch {trial $action}]}
    check {$protections eq {} && $erases eq {}}
    check {$core_state eq "halted" && $resets == 0}
}

start_trial $stock_app $probe_header
set fail_reprotect 1
check {[catch {trial restore} message] && [string match {*without a reset*} $message]}
check {$resets == 0 && $sr1 == 0}

# A lost debugger connection may leave the already validated image unlocked.
foreach {header expected_erases} [list $partial {0} $stock_header {}] {
    start_trial $stock_app $header
    set sr1 0
    trial restore
    check {$erases eq $expected_erases && $protections eq {0x7c}}
    check {$sr1 == 0x7c && $resets == 1}
}
foreach {action initial header metadata} [list \
        install 0 $stock_header $ota install 2 $stock_header $ota \
        restore 0x3c $stock_header $ota restore 0 $unknown $ota \
        restore 0 $stock_header $unknown] {
    start_trial $stock_app $header
    set sr1 $initial
    set sectors(4096) $metadata
    check {[catch {trial $action}]}
    check {$protections eq {} && $erases eq {} && $resets == 0}
}
# Reconnect after WREN, before erase. Keep the latch on a package mismatch.
foreach {app matches} [list $unknown 0 $probe_app 1] {
    start_trial $app $probe_header
    set sr1 0
    stig 0x06000000
    if {$matches} {
        trial restore
        check {$erases eq {8192 0} && $resets == 1 && $sr1 == 0x7c && !$wel}
        check {[same_words $sectors(8192) $stock_app] && [same_words $sectors(0) $stock_header]}
    } else {
        check {[catch {trial restore}]}
        check {$wel == 2 && $protections eq {} && $erases eq {} && $resets == 0}
        check {[same_words $sectors(8192) $app] && [same_words $sectors(0) $probe_header]}
    }
}
# First WREN, both installed sectors still contain the probe. Two internally
# consistent packages differ only in backup identity on their candidate side.
start_trial $probe_app $probe_header
set sr1 0
stig 0x06000000
set other_stock $stock
set other_fixture [model_backup other]
dict set other_stock backup_digest [lrange $other_fixture 1 end]
dict set other_stock backup_file [lindex $other_fixture 0]
dict set other_stock 0x2000 [lreplace $stock_app 10 10 0x0123]
set other_candidate $candidate
dict set other_candidate 0x0 [lreplace $probe_header 56 63 {*}[dict get $other_stock backup_digest]]
check {[catch {flash_trial restore $other_stock $other_candidate $ota $helper} msg]}
check {[string match {*Installed backup marker*} $msg]}
check {$erases eq {} && $protections eq {} && $wel == 2 && $resets == 0}
check {[same_words $sectors(8192) $probe_app] && [same_words $sectors(0) $probe_header]}
trial restore
check {[same_words $sectors(8192) $stock_app] && [same_words $sectors(0) $stock_header]}
check {$erases eq {8192 0} && $sr1 == 0x7c && !$wel && $resets == 1}
puts "Interrupted restore, idempotence and preflight guards passed."

# Second WREN: application already restored, header still carries the tag.
start_trial $stock_app $probe_header
set sr1 0
stig 0x06000000
check {[catch {flash_trial restore $other_stock $other_candidate $ota $helper}]}
check {$erases eq {} && $wel == 2 && $protections eq {}}
trial restore
check {$erases eq {0} && [same_words $sectors(0) $stock_header] && $sr1 == 0x7c}

# A header erase also removes the Flash marker; refuse ambiguous writes if
# power loss has removed the transaction's SRAM witness.
start_trial $stock_app $partial
set witness [lrepeat 9 0]
check {[catch {trial restore} msg] && [string match {*Retained backup marker*} $msg]}
check {$erases eq {} && $protections eq {} && $resets == 0}

# Both outgoing sectors must match the hashed file before clearing WEL.
foreach offset {0x0 0x2000} {
    start_trial $probe_app $probe_header
    set sr1 0
    stig 0x06000000
    set damaged $stock
    set words [dict get $stock $offset]
    dict set damaged $offset [lreplace $words 32 32 [expr {[lindex $words 32] ^ 1}]]
    check {[catch {flash_trial restore $damaged $candidate $ota $helper}]}
    check {$wel == 2 && $erases eq {} && $protections eq {} && $resets == 0}
    check {[same_words $sectors(8192) $probe_app] && [same_words $sectors(0) $probe_header]}
}
# Interrupted install, before a Flash marker exists. Only the powered
# transaction's matching SRAM marker authorizes recovery.
set blank [lrepeat 1024 0xffffffff]
set probe_prefix [concat [lrange $probe_app 0 63] [lrepeat 960 0xffffffff]]
foreach {app header} [list $probe_prefix $stock_header $probe_app $blank] {
    foreach identity {wrong missing correct} {
        start_trial $app $header
        set sr1 0
        stig 0x06000000
        if {$identity eq "correct"} {
            trial restore
            check {[same_words $sectors(8192) $stock_app] && [same_words $sectors(0) $stock_header]}
            check {$sr1 == 0x7c && !$wel && $resets == 1}
        } else {
            if {$identity eq "missing"} { set witness [lrepeat 9 0] }
            if {$identity eq "wrong"} { set witness [linsert [dict get $other_stock backup_digest] 0 0x3130464d] }
            check {[catch {trial restore} msg] && [string match {*Retained backup marker*} $msg]}
            check {$wel == 2 && $erases eq {} && $protections eq {} && $resets == 0}
            check {[same_words $sectors(8192) $app] && [same_words $sectors(0) $header]}
        }
    }
}
file delete [dict get $stock backup_file] [dict get $other_stock backup_file]
