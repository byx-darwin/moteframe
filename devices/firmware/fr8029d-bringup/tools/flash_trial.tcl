# FR8029D/Puya P25Q80 two-sector experiment. Requires a prepared package.
# Sourcing this file defines operations; it does not attach or program.

proc require_sector {offset words} {
    if {($offset != 0 && $offset != 0x2000) || [llength $words] != 1024} {
        error "Invalid Flash sector or payload length"
    }
}
proc require_page {offset} {
    if {$offset < 0 || ($offset & 0xff) != 0 ||
        !(($offset <= 0xf00) || ($offset >= 0x2000 && $offset <= 0x2f00))} {
        error "Page outside the two approved sectors"
    }
}
proc word {address} {
    return [lindex [read_memory $address 32 1] 0]
}
proc put_word {address value} {
    write_memory $address 32 [list $value]
}
proc same_words {actual expected} {
    if {[llength $actual] != [llength $expected]} { return 0 }
    foreach got $actual wanted $expected {
        if {$got != $wanted} { return 0 }
    }
    return 1
}
proc verify_words {label actual expected} {
    if {![same_words $actual $expected]} { error "$label verification failed" }
}
proc verify_diagnostic {words} {
    # Bit 31 reports QSPI idle, so its value can change with XIP instruction fetch.
    set config [expr {[lindex $words 5] & 0x7fffffff}]
    set normalized [lreplace $words 5 5 $config]
    verify_words "XIP diagnostic" $normalized {0x39325246 1 1 0x410fd210 0x10002000 0x00890087 0x00101002 0}
}

proc wait_word {address mask expected} {
    for {set i 0} {$i < 100} {incr i} {
        if {([word $address] & $mask) == $expected} { return }
        sleep 1
    }
    error [format "Register %#x did not reach idle state" $address]
}
proc stig {command} {
    wait_word 0xe00a4000 0x80000000 0x80000000
    wait_word 0xe00a4090 3 0
    put_word 0xe00a4090 $command
    put_word 0xe00a4090 [expr {$command | 1}]
    wait_word 0xe00a4090 3 0
}
proc status1 {} {
    stig 0x05800000
    return [expr {[word 0xe00a40a0] & 0xff}]
}
proc status2 {} {
    stig 0x35800000
    return [expr {[word 0xe00a40a0] & 0xff}]
}
proc ready {} {
    for {set i 0} {$i < 100} {incr i} {
        set value [status1]
        if {($value & 1) == 0} { return $value }
        sleep 1
    }
    error "Flash WIP remained set"
}
proc set_volatile_protection {configuration} {
    if {$configuration != 0 && $configuration != 0x7c} { error "Invalid protection value" }
    set old [ready]
    if {$old & 2} {
        stig 0x04000000
        if {[status1] & 2} { error "Flash WEL remained set" }
    }
    if {[status2] != 2} { error "Unexpected Flash SR2 configuration" }
    # 0x50 arms a volatile status write only. Keep SR2 and QE unchanged.
    stig 0x50000000
    put_word 0xe00a40a8 $configuration
    stig 0x01008000
    if {[ready] != $configuration || [status2] != 2} {
        error "Volatile Flash protection readback failed"
    }
}
proc read_sector {offset} {
    return [read_memory [expr {0x10000000 + $offset}] 32 1024]
}

proc ensure_watchdogs_idle {} {
    # PMU SPI read, one byte from IWDT control, with all write bits clear.
    if {[word 0xe0060000] & 1} { error "The system watchdog is enabled" }
    wait_word 0xe0080000 1 0
    put_word 0xe0080000 0x00100011
    wait_word 0xe0080000 3 2
    if {[word 0xe0080008] & 1} { error "The PMU watchdog is enabled" }
}
proc program_page {offset data} {
    require_page $offset
    if {[llength $data] != 64} { error "Expected an aligned 256-byte page" }
    write_memory 0x20004000 32 $data
    verify_words "SRAM page buffer" [read_memory 0x20004000 32 64] $data
    if {[ready] != 0} { error "Flash not unlocked and idle before page write" }
    set_reg [dict create r0 $offset xPSR 0x01000000 control 0 msp 0x20008000 primask 1]
    resume 0x20003000
    wait_halt 1000
    set regs [get_reg -force {r0 pc xPSR}]
    set pc [dict get $regs pc]
    if {$pc != 0x20003032 ||
        [dict get $regs r0] != 0 || ([dict get $regs xPSR] & 0x1ff) != 0} {
        error "SRAM page writer did not reach its completion breakpoint"
    }
    if {[ready] != 0} { error "Unexpected status after Flash page write" }
    verify_words "Flash page" [read_memory [expr {0x10000000 + $offset}] 32 64] $data
}
proc burn_sector {offset words} {
    require_sector $offset $words
    if {[same_words [read_sector $offset] $words]} { return }
    if {[ready] != 0} { error "Flash not unlocked and idle before erase" }
    stig 0x06000000
    if {[status1] != 2} { error "Flash WREN not acknowledged" }
    put_word 0xe00a4094 $offset
    stig 0x200a0000
    if {[ready] != 0} { error "Sector erase not completed" }
    set blank [lrepeat 64 0xffffffff]
    verify_words "Erased sector" [read_sector $offset] [lrepeat 1024 0xffffffff]
    for {set index 0} {$index < 1024} {incr index 64} {
        set page [lrange $words $index [expr {$index + 63}]]
        if {![same_words $page $blank]} {
            program_page [expr {$offset + 4*$index}] $page
        }
    }
    verify_words "Written sector" [read_sector $offset] $words
    echo [format "FLASH_SECTOR_VERIFIED %#06x" $offset]
}
proc write_pair {pair} {
    set app [dict get $pair 0x2000]
    set header [dict get $pair 0x0]
    require_sector 0x2000 $app
    require_sector 0x0 $header
    burn_sector 0x2000 $app
    burn_sector 0 $header
}
proc install_pair {original candidate} {
    set ::rollback_needed 1
    if {[catch {write_pair $candidate} failure]} {
        if {[catch {halt 2000; ready; write_pair $original} rollback_error]} {
            error "Install failed: $failure; rollback unfinished: $rollback_error"
        }
        set ::rollback_needed 0
        error "Install failed and original sectors were restored: $failure"
    }
}

proc flash_trial {action original candidate ota helper} {
    if {$action ni {install restore}} { error "Unknown action" }
    if {[llength $ota] != 1024 || [llength $helper] < 4 || [llength $helper] > 64} {
        error "Invalid package data"
    }
    foreach image [list $original $candidate] {
        require_sector 0x2000 [dict get $image 0x2000]
        require_sector 0x0 [dict get $image 0x0]
    }
    if {[word 0xe000ed00] != 0x410fd210} { error "CPU doesn't match the FR8029D board" }
    set installed_image $original
    if {$action eq "restore"} { set installed_image $candidate }

    set ::rollback_needed [expr {$action eq "restore"}]
    set protection_touched 0
    set trial_error [catch {
        halt 2000
        if {([dict get [get_reg -force {xPSR}] xPSR] & 0x1ff) != 0} {
            error "CPU stopped in an exception"
        }
        ensure_watchdogs_idle
        set qspi [read_memory 0xe00a4000 32 10]
        set stock_qspi {0x80890087 0x0802006b 2 0x08000404 5 0x00101002 0 0 0 0}
        set rom_qspi {0x80890087 0x0801003b 2 0x00000202 1 0x00101002 0 0 0 0}
        if {![same_words $qspi $stock_qspi] && ![same_words $qspi $rom_qspi]} {
            error "QSPI configuration differs from stock firmware and ROM handoff"
        }
        verify_words "QSPI controller protection" [read_memory 0xe00a4050 32 3] {0 0 0}
        stig 0x9fa00000
        if {([word 0xe00a40a0] & 0xffffff) != 0x146085 || [ready] != 0x7c || [status2] != 2} {
            error "Flash JEDEC or starting protection differs"
        }

        write_memory 0x20003000 32 $helper
        verify_words "SRAM helper" [read_memory 0x20003000 32 [llength $helper]] $helper
        put_word 0xe00a0000 0x3c
        put_word 0xe00a0000 0x38
        wait_word 0xe00a0004 3 0

        verify_words "OTA sector" [read_sector 0x1000] $ota
        verify_words "Starting header" [read_sector 0] [dict get $installed_image 0x0]
        verify_words "Starting application" [read_sector 0x2000] [dict get $installed_image 0x2000]
        set protection_touched 1
        set_volatile_protection 0
        if {$action eq "install"} {
            install_pair $original $candidate
        } else {
            write_pair $original
            set ::rollback_needed 0
        }
        verify_words "Unchanged OTA sector" [read_sector 0x1000] $ota
    } trial_result]

    set protected 1
    if {$protection_touched} {
        if {[catch {halt 2000; set_volatile_protection 0x7c} protection_error]} {
            set protected 0
            echo "FLASH_PROTECTION_UNVERIFIED $protection_error"
        }
    }
    if {!$protected || ($trial_error && $::rollback_needed)} {
        error "Transaction stopped without a reset; inspect the board before resuming: $trial_result"
    }
    if {$trial_error} {
        reset run
        error $trial_result
    }
    if {$action eq "install"} {
        write_memory 0x20006f00 32 {0 0 0 0 0 0 0 0}
        echo "FLASH_PROBE_WRITTEN_AND_PROTECTED"
    } else {
        echo "FLASH_ORIGINAL_WRITTEN_AND_PROTECTED"
    }
    reset run

    if {$action eq "install"} {
        set diagnostic {}
        for {set i 0} {$i < 20} {incr i} {
            sleep 100
            set diagnostic [read_memory 0x20006f00 32 8]
            if {[lindex $diagnostic 0] == 0x39325246} { break }
        }
        echo "FLASH_DIAGNOSTIC $diagnostic"
        verify_diagnostic $diagnostic
    } else {
        sleep 500
    }
    verify_words "VTOR after reset" [read_memory 0xe000ed08 32 1] {0x10002000}
    set expected [expr {$action eq "install" ? $candidate : $original}]
    verify_words "Vectors after reset" [read_memory 0x10002000 32 2] \
        [lrange [dict get $expected 0x2000] 0 1]
    echo "FR8029D_FLASH_TRIAL_OK $action"
}
