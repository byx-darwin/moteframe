# Source the output of gatt_notify_memory.py pack before this file.
# Run one temporary trial with: run_notify_trial 240 /path/to/stock-prefix.bin

proc notify_core_check {} {
    if {[read_memory 0xe000ed00 32 1] != 0x410fd210 ||
        [read_memory 0x10002000 32 2] ne {0x2000b058 0x10002121}} {
        error "Unexpected FR8029D board or stock reset vectors"
    }
}

proc notify_sram_check {address size} {
    if {$size <= 0 || $address % 4 || $size % 4 ||
        $address < 0x20003000 || $address + $size > 0x20018000} {
        error "Write would leave the FR8029D application SRAM window"
    }
}

proc notify_sram_write_checked {address words} {
    set count [llength $words]
    notify_sram_check $address [expr {$count * 4}]
    write_memory $address 32 $words
    set actual [read_memory $address 32 $count]
    if {$actual ne $words} {
        if {[llength $actual] != $count} {error "Short SRAM readback at $address"}
        foreach got $actual expected $words {
            if {$got != $expected} {error "SRAM readback differs at $address"}
            incr address 4
        }
    }
}

proc notify_stack_touched {base size paint} {
    set words [read_memory $base 32 [expr {$size / 4}]]
    if {[llength $words] * 4 != $size} {error "Short stack watermark read at $base"}
    set untouched 0
    foreach word $words {
        if {$word != $paint} {break}
        incr untouched 4
    }
    return [expr {$size - $untouched}]
}

proc notify_diag {} {
    return [read_memory [expr {$::notify_diag + 40 * 4}] 32 17]
}

proc notify_wait_advertising {} {
    for {set i 0} {$i < 60} {incr i} {
        set diag [notify_diag]
        set phase [lindex $diag 0]
        if {$phase == 2 || $phase == 3 || $phase == 4} {
            if {[read_memory 0xe000ed08 32 1] != $::notify_vector} {
                error "Unexpected trial vector table"
            }
            echo "NOTIFY_ADVERTISING $::notify_identity $diag"
            return
        }
        if {$phase & 0x80000000} {error "GATT startup failed with diagnostics $diag"}
        sleep 50
    }
    error "GATT startup timed out"
}

proc notify_check_scheduler {} {
    if {[dict exists $::notify_runtime_words status]} {
        set status [read_memory [dict get $::notify_runtime_words status] 32 1]
        echo "FREERTOS_STATUS $status"
        if {$status != 1} {error "FreeRTOS scheduler did not start"}
    }
    if {[dict exists $::notify_runtime_words tick]} {
        set tick_address [dict get $::notify_runtime_words tick]
        set before [read_memory $tick_address 32 1]
        sleep 30
        set after [read_memory $tick_address 32 1]
        echo "FREERTOS_TICK $before $after"
        if {$after == $before} {error "FreeRTOS tick did not advance"}
    }
}

proc notify_wait_for_tx {seconds} {
    set last {}
    set deadline [expr {[clock seconds] + $seconds}]
    set observed 0
    set abandoned 0
    set disconnects 0
    while {[clock seconds] < $deadline} {
        set d [notify_diag]
        set phase [lindex $d 0]
        set outstanding [expr {[lindex $d 9] - [lindex $d 10]}]
        if {[lindex $d 5] != $disconnects} {
            set disconnects [lindex $d 5]
            set observed 0
        }
        if {$phase == 2} {
            # Counters are cumulative. A disconnected link can leave unacked
            # sends in the history even after its queue has been discarded.
            set abandoned $outstanding
        }
        set key [list $phase [lindex $d 1] [lindex $d 13] [lindex $d 14]]
        if {$key ne $last} {
            echo "NOTIFY_DIAG $d"
            if {$last ne {} && [lindex $key 3] != [lindex $last 3]} {
                echo "ATT_TX [read_memory 0x40011984 8 15]"
            }
            set last $key
        }
        if {$phase & 0x80000000} {error "GATT stopped with diagnostics $d"}
        if {!$observed && [lindex $d 1] >= 1 && [lindex $d 4] == 9 && [lindex $d 16] == 27} {
            set expected {0xb 0x0 0x4 0x0 0x1b 0x9 0x0}
            foreach word [lrange $d 2 3] {
                for {set shift 0} {$shift < 32} {incr shift 8} {
                    lappend expected [format 0x%x [expr {($word >> $shift) & 0xff}]]
                }
            }
            if {[read_memory 0x40011984 8 15] eq $expected} {
                echo "PHONE_NOTIFICATION_VERIFIED $expected"
                set observed 1
            }
        }
        if {$observed && $outstanding == $abandoned} {
            echo "TX_QUEUE_CONFIRMED $d"
            return
        }
        sleep 10
    }
    error "Timed out waiting for expected ATT Handle Value Notification and TX completion"
}

proc notify_reset_stock {} {
    reset run
    sleep 500
    notify_core_check
    if {[read_memory 0xe000ed08 32 1] != 0x10002000} {
        error "Original firmware did not restart at its Flash vector table"
    }
}

# Recovery after an interrupted OpenOCD session. The prefix must be the original
# dump captured at the beginning of that trial.
proc restore_notify_stock {prefix_path} {
    if {![file isfile $prefix_path] || [file size $prefix_path] != 12288} {
        error "Expected the 12288-byte original Flash prefix capture"
    }
    notify_core_check
    notify_reset_stock
    verify_image $prefix_path 0x10000000 bin
    echo "STOCK_RESTORED FLASH_PREFIX_BYTES=12288"
}

proc run_notify_trial {seconds {prefix_path {}}} {
    if {![string is integer -strict $seconds] || $seconds <= 0} {
        error "Provide a positive trial duration in seconds"
    }
    notify_core_check
    if {[read_memory 0xe000ed08 32 1] != 0x10002000} {
        error "Expected starting firmware to be stock"
    }
    set prefix [read_memory 0x10000000 32 3072]
    if {[llength $prefix] != 3072} {error "Short original Flash prefix read"}
    if {$prefix_path ne {}} {
        if {[file exists $prefix_path]} {error "Original Flash prefix output already exists"}
        dump_image $prefix_path 0x10000000 12288
    }

    echo "NOTIFY_TRIAL $::notify_identity ELF_SHA256=$::notify_elf_sha256"
    set breakpoint_installed 0
    set resumed 0
    set status [catch {
        halt 2000
        bp 0x10002120 2 hw
        set breakpoint_installed 1
        reset run
        wait_halt 10000
        if {[dict get [get_reg -force {pc}] pc] != 0x10002120 ||
            [read_memory 0x40000004 32 1] != 0} {
            error "Expected reset halt before stock FR8029D radio initialization"
        }
        rbp 0x10002120
        set breakpoint_installed 0

        set msp_top 0
        foreach stack $::notify_stacks {
            lassign $stack label base size paint
            notify_sram_check $base $size
            notify_sram_write_checked $base [lrepeat [expr {$size / 4}] $paint]
            if {$label eq "msp"} {set msp_top [expr {$base + $size}]}
        }
        foreach region $::notify_regions {
            lassign $region address words
            notify_sram_write_checked $address $words
            echo "NOTIFY_LOAD_VERIFIED $address [llength $words] words"
        }
        notify_sram_write_checked $::notify_diag [lrepeat 64 0]
        if {$msp_top == 0} {error "Missing validated MSP region"}
        set_reg [dict create xPSR 0x01000000 control 0 msp $msp_top primask 1]
        resume $::notify_entry
        set resumed 1

        notify_wait_advertising
        notify_check_scheduler
        notify_wait_for_tx $seconds
        # Keep the link open for viewing the notification on the phone.
        sleep 30000
    } explanation]
    if {$resumed} {
        set sample_failure [catch {
            halt 1500
            echo "NOTIFY_FINAL [notify_diag]"
            foreach stack $::notify_stacks {
                lassign $stack label base size paint
                set touched [notify_stack_touched $base $size $paint]
                echo "STACK_WATERMARK $label RESERVE=$size TOUCHED=$touched"
            }
        } sample_error]
        if {$sample_failure} {
            echo "STACK_WATERMARK_FAILED $sample_error"
            if {!$status} {
                set status $sample_failure
                set explanation $sample_error
            }
        }
    }
    if {$status} {
        if {$breakpoint_installed} {catch {rbp 0x10002120}}
        catch {halt 1000}
        catch {echo "NOTIFY_FAILURE_REGS [get_reg -force {pc msp psp xPSR primask}]"}
        catch {echo "NOTIFY_FAILURE_DIAG [read_memory $::notify_diag 32 64]"}
        echo "NOTIFY_FAILURE $explanation"
    }
    notify_reset_stock
    if {[read_memory 0x10000000 32 3072] ne $prefix} {
        error "Original Flash prefix readback differs"
    }
    echo "STOCK_RESTORED FLASH_PREFIX_BYTES=12288"
    if {$status} {error $explanation}
}
