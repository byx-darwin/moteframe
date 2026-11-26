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
    check {$address == 0x20003000}
    set ::helper_ram $data
}
proc read_memory {address width count} {
    switch -- $address {
        0xe00a4000 { return {0x80890087 0x0802006b 2 0x08000404 5 0x00101002 0 0 0 0} }
        0xe00a4050 { return {0 0 0} }
        0x20003000 { return $::helper_ram }
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
check {$sectors(8192) eq $stock_app}
check {[lindex $sectors(0) 127] == 0x1234 && [lindex $sectors(0) 128] == 0xffffffff}
check {$sr1 == 0x7c && $resets == 0}
set erases {}
set fail_page -1
trial restore
check {$erases eq {0} && $sectors(0) eq $stock_header && $sectors(4096) eq $ota}
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
    check {$erases eq $expected_erases && $sectors(8192) eq $stock_app && $sectors(0) eq $stock_header}
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
        check {$sectors(8192) eq $stock_app && $sectors(0) eq $stock_header}
    } else {
        check {[catch {trial restore}]}
        check {$wel == 2 && $protections eq {} && $erases eq {} && $resets == 0}
        check {$sectors(8192) eq $app && $sectors(0) eq $probe_header}
    }
}
puts "Interrupted restore, idempotence and preflight guards passed."
