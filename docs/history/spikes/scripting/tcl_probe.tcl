# Throwaway probe of Tcl safe interpreters (Git for Windows ships Tcl 8.6). Run: timeout 60 tclsh tcl_probe.tcl
puts "Tcl [info patchlevel] on [set tcl_platform(os)]"

proc attempt {label script {limitkind none} {limitvalue 0}} {
    set safe [interp create -safe]
    $safe eval {proc range {n} {set r {}; for {set k 0} {$k < $n} {incr k} {lappend r $k}; return $r}}
    switch $limitkind {
        command { interp limit $safe command -value $limitvalue -granularity 1 }
        time    { interp limit $safe time -seconds [expr {[clock seconds] + $limitvalue}] -granularity 1 }
    }
    set started [clock microseconds]
    set code [catch {$safe eval $script} result]
    set elapsed [expr {([clock microseconds] - $started) / 1000.0}]
    set shown [string range [string map {"\n" " / "} $result] 0 90]
    puts [format "%-50s code=%d %-95s (%.1f ms)" $label $code $shown $elapsed]
    interp delete $safe
}

puts "\n== generator-style forms"
attempt "lmap with continue as filter (4096 -> 2048)" {llength [lmap x [range 4096] {if {$x % 2} {continue}; set x}]}
attempt "nested foreach cube, even sum, count" {
    set n 0
    foreach x [range 16] {foreach y [range 16] {foreach z [range 16] {if {($x + $y + $z) % 2 == 0} {incr n}}}}
    set n
}
attempt "nested lmap flatten, count" {
    llength [concat {*}[concat {*}[lmap x [range 16] {lmap y [range 16] {lmap z [range 16] {if {($x + $y + $z) % 2} {continue}; list $x $y $z}}}]]]
}
attempt "coroutine generator, take 5" {
    proc nat {} {yield [info coroutine]; set i 0; while 1 {yield $i; incr i}}
    coroutine g nat
    g; set out {}
    for {set k 0} {$k < 5} {incr k} {lappend out [g]}
    set out
}
attempt "dict map" {dict map {k v} {a 1 b 2 c 3} {expr {$v * $v}}}

puts "\n== sandbox: hidden commands"
attempt "visible: open exec file socket source load cd pwd exit" {
    set found {}
    foreach c {open exec file socket source load cd pwd exit glob encoding} {if {[llength [info commands $c]]} {lappend found $c}}
    set found
}
attempt "try open a file" {open "C:/Windows/win.ini"}
attempt "try exec" {exec cmd /c echo hi}
attempt "clock is visible? (clock seconds)" {clock seconds}
attempt "rand unseeded x2" {list [expr {rand()}] [expr {rand()}]}
attempt "sin/exp/pow" {list [expr {sin(1.0)}] [expr {exp(1.0)}] [expr {pow(2.5, 3.7)}] [expr {sin(1e22)}]}

puts "\n== limits"
attempt "while 1 {} with command limit 1000" {while 1 {}} command 1000
attempt "for loop with incr, command limit 1000" {for {set k 0} {1} {incr k} {}} command 1000
attempt "loop calling a proc 1M times, command limit 10000" {proc f {x} {return $x}; for {set k 0} {$k < 1000000} {incr k} {f $k}} command 10000
attempt "plain loop of 100k iterations, command limit 1000" {set s 0; for {set k 0} {$k < 100000} {incr k} {incr s $k}; set s} command 1000
attempt "while 1 {} with time limit 1 s" {while 1 {}} time 1
attempt "recursion unbounded" {proc r {n} {r [expr {$n + 1}]}; r 0}
attempt "memory bomb: string doubling x40" {set s a; for {set k 0} {$k < 40} {incr k} {append s $s}; string length $s}
