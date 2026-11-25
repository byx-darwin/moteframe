# Tests exercise host-side range validation and recovery using a NOR model.
source [file join [file dirname [info script]] flash_trial.tcl]

proc check {condition} {
    if {![uplevel 1 [list expr $condition]]} { error "assertion failed: $condition" }
}
proc halt {timeout} {}
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
