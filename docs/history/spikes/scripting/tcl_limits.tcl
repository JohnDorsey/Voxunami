# One limit case per process, chosen by argv, so a hang can be killed from outside. Run: timeout 8 tclsh tcl_limits.tcl N
set cases {
    1 {"while 1 {} , command limit 1000" {while 1 {}} command 1000}
    2 {"for {} {1} {incr k} {}, command limit 1000" {for {set k 0} {1} {incr k} {}} command 1000}
    3 {"1M proc calls, command limit 10000" {proc f {x} {return $x}; for {set k 0} {$k < 1000000} {incr k} {f $k}} command 10000}
    4 {"100k plain loop, command limit 1000" {set s 0; for {set k 0} {$k < 100000} {incr k} {incr s $k}; set s} command 1000}
    5 {"while 1 {} , time limit 1 s" {while 1 {}} time 1}
    6 {"for/incr loop, time limit 1 s" {for {set k 0} {1} {incr k} {}} time 1}
    7 {"unbounded recursion, no limit" {proc r {n} {r [expr {$n + 1}]}; r 0}}
    8 {"while 1 {set x 1}, command limit 1000" {while 1 {set x 1}} command 1000}
    9 {"while 1 {expr {1+1}}, command limit 1000" {while 1 {expr {1+1}}} command 1000}
}
lassign [dict get $cases [lindex $argv 0]] label script limitkind limitvalue
set safe [interp create -safe]
switch -- $limitkind {
    command { interp limit $safe command -value $limitvalue -granularity 1 }
    time    { interp limit $safe time -seconds [expr {[clock seconds] + $limitvalue}] -granularity 1 }
}
set started [clock microseconds]
set code [catch {$safe eval $script} result]
set elapsed [expr {([clock microseconds] - $started) / 1000.0}]
puts [format "%-46s code=%d %-70s (%.1f ms)" $label $code [string range [string map {"\n" " / "} $result] 0 70] $elapsed]
