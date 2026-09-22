// Differential oracle for cronslop.
//
// Reads cron expressions on stdin and, for each, reports what
// robfig/cron v3 -- the parser Kubernetes validates CronJobs with -- does
// with it: either ERR, or the largest gap between consecutive firings
// found by walking the schedule forward with Next().
//
// The walk is the ground truth cronslop's closed-form arithmetic is
// checked against. It runs in UTC, matching cronslop's stated scope.
package main

import (
	"bufio"
	"fmt"
	"os"
	"time"

	"github.com/robfig/cron/v3"
)

const (
	// Wide enough to contain two skipped century leap years (2100, 2200),
	// which is what a Feb-29 schedule actually waits for.
	simStart = 2000
	simEnd   = 2300
	// Cap for frequent schedules, whose worst gap repeats almost at once.
	maxFirings = 200000
)

// parse reports the schedule, recovering from the panics ParseStandard can
// raise on malformed input -- exactly as Kubernetes' own wrapper does.
func parse(expr string) (sched cron.Schedule, err error) {
	defer func() {
		if r := recover(); r != nil {
			err = fmt.Errorf("panic: %v", r)
		}
	}()
	return cron.ParseStandard(expr)
}

// maxGap walks the schedule and returns the largest gap in seconds.
func maxGap(sched cron.Schedule) int64 {
	// Force UTC so the comparison matches cronslop's scope.
	if spec, ok := sched.(*cron.SpecSchedule); ok {
		spec.Location = time.UTC
	}
	start := time.Date(simStart, 1, 1, 0, 0, 0, 0, time.UTC)
	end := time.Date(simEnd, 1, 1, 0, 0, 0, 0, time.UTC)

	prev := sched.Next(start)
	if prev.IsZero() {
		return 0 // never fires
	}
	var worst int64
	for n := 0; n < maxFirings; n++ {
		next := sched.Next(prev)
		if next.IsZero() || next.After(end) {
			break
		}
		if gap := int64(next.Sub(prev).Seconds()); gap > worst {
			worst = gap
		}
		prev = next
	}
	return worst
}

func main() {
	in := bufio.NewScanner(os.Stdin)
	in.Buffer(make([]byte, 1024*1024), 1024*1024)
	out := bufio.NewWriter(os.Stdout)
	defer out.Flush()

	for in.Scan() {
		expr := in.Text()
		sched, err := parse(expr)
		if err != nil {
			fmt.Fprintf(out, "%s\tERR\n", expr)
			continue
		}
		fmt.Fprintf(out, "%s\tOK %d\n", expr, maxGap(sched))
	}
}
