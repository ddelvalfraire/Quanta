package main

import (
	"encoding/json"
	"fmt"
	"math"
	"os"
	"sort"
	"sync"
	"sync/atomic"
	"time"

	"github.com/asynkron/protoactor-go/actor"
)

// ---------------------------------------------------------------------------
// Standardized iteration counts
// ---------------------------------------------------------------------------

const (
	iterPingPong1k    = 200
	iterPingPong10k   = 100
	iterFanOut10      = 200
	iterFanOut100     = 100
	iterFanOut1000    = 50
	iterSkynet        = 10
	iterColdActivation = 1000
	iterWarmMessage   = 1000

	warmupStandard = 20
	warmupSkynet   = 5
)

// ---------------------------------------------------------------------------
// Message types
// ---------------------------------------------------------------------------

type ping struct{}
type pong struct{}
type broadcast struct{ replyTo *actor.PID }
type ack struct{}
type skynetMsg struct {
	num  int64
	size int64
}
type skynetResult struct{ sum int64 }
type warmPing struct{}
type warmPong struct{}

// ---------------------------------------------------------------------------
// Statistics helpers
// ---------------------------------------------------------------------------

type stats struct {
	Iterations int     `json:"iterations"`
	MeanUs     float64 `json:"mean_us"`
	MedianUs   float64 `json:"median_us"`
	MinUs      float64 `json:"min_us"`
	MaxUs      float64 `json:"max_us"`
	P99Us      float64 `json:"p99_us"`
	Ips        float64 `json:"ips"`
}

func computeStats(durations []time.Duration) stats {
	n := len(durations)
	if n == 0 {
		return stats{}
	}

	us := make([]float64, n)
	for i, d := range durations {
		us[i] = float64(d.Microseconds())
	}
	sort.Float64s(us)

	sum := 0.0
	for _, v := range us {
		sum += v
	}
	mean := sum / float64(n)
	median := percentile(us, 50)
	p99 := percentile(us, 99)

	ips := 0.0
	if mean > 0 {
		ips = 1_000_000.0 / mean
	}

	return stats{
		Iterations: n,
		MeanUs:     math.Round(mean*100) / 100,
		MedianUs:   math.Round(median*100) / 100,
		MinUs:      math.Round(us[0]*100) / 100,
		MaxUs:      math.Round(us[n-1]*100) / 100,
		P99Us:      math.Round(p99*100) / 100,
		Ips:        math.Round(ips*100) / 100,
	}
}

func percentile(sorted []float64, pct float64) float64 {
	if len(sorted) == 0 {
		return 0
	}
	rank := (pct / 100.0) * float64(len(sorted)-1)
	lower := int(math.Floor(rank))
	upper := int(math.Ceil(rank))
	if lower == upper {
		return sorted[lower]
	}
	frac := rank - float64(lower)
	return sorted[lower]*(1-frac) + sorted[upper]*frac
}

// ---------------------------------------------------------------------------
// Benchmark: Ping-Pong
// ---------------------------------------------------------------------------

func benchPingPong(system *actor.ActorSystem, roundTrips int, warmup int, iterations int) stats {
	allDurations := make([]time.Duration, 0, warmup+iterations)

	for i := 0; i < warmup+iterations; i++ {
		var wg sync.WaitGroup
		wg.Add(1)

		var remaining int64 = int64(roundTrips)
		var pongerPID *actor.PID

		pingerProps := actor.PropsFromFunc(func(c actor.Context) {
			switch c.Message().(type) {
			case *actor.Started:
				// use Request so ponger can Respond back to us
				c.Request(pongerPID, &ping{})
			case *pong:
				left := atomic.AddInt64(&remaining, -1)
				if left <= 0 {
					wg.Done()
				} else {
					c.Request(pongerPID, &ping{})
				}
			}
		})

		pongerProps := actor.PropsFromFunc(func(c actor.Context) {
			switch c.Message().(type) {
			case *ping:
				c.Respond(&pong{})
			}
		})

		pongerPID = system.Root.Spawn(pongerProps)

		start := time.Now()
		pingerPID := system.Root.Spawn(pingerProps)
		wg.Wait()
		elapsed := time.Since(start)

		_ = system.Root.StopFuture(pingerPID).Wait()
		_ = system.Root.StopFuture(pongerPID).Wait()

		allDurations = append(allDurations, elapsed)
	}

	// Discard warmup iterations
	return computeStats(allDurations[warmup:])
}

// ---------------------------------------------------------------------------
// Benchmark: Fan-Out
// ---------------------------------------------------------------------------

func benchFanOut(system *actor.ActorSystem, fanSize int, warmup int, iterations int) stats {
	allDurations := make([]time.Duration, 0, warmup+iterations)

	for i := 0; i < warmup+iterations; i++ {
		var wg sync.WaitGroup
		wg.Add(1)

		var ackCount int64

		collectorProps := actor.PropsFromFunc(func(c actor.Context) {
			switch c.Message().(type) {
			case *ack:
				count := atomic.AddInt64(&ackCount, 1)
				if count >= int64(fanSize) {
					wg.Done()
				}
			}
		})

		workerProps := actor.PropsFromFunc(func(c actor.Context) {
			switch msg := c.Message().(type) {
			case *broadcast:
				system.Root.Send(msg.replyTo, &ack{})
			}
		})

		collectorPID := system.Root.Spawn(collectorProps)

		// Worker spawning is inside the timed section
		start := time.Now()
		workers := make([]*actor.PID, fanSize)
		for j := 0; j < fanSize; j++ {
			workers[j] = system.Root.Spawn(workerProps)
		}
		msg := &broadcast{replyTo: collectorPID}
		for _, w := range workers {
			system.Root.Send(w, msg)
		}
		wg.Wait()
		elapsed := time.Since(start)

		// Cleanup
		atomic.StoreInt64(&ackCount, 0)
		_ = system.Root.StopFuture(collectorPID).Wait()
		for _, w := range workers {
			_ = system.Root.StopFuture(w).Wait()
		}

		allDurations = append(allDurations, elapsed)
	}

	// Discard warmup iterations
	return computeStats(allDurations[warmup:])
}

// ---------------------------------------------------------------------------
// Benchmark: Skynet
// ---------------------------------------------------------------------------

type skynetActorImpl struct {
	system   *actor.ActorSystem
	sum      int64
	received int
	parent   *actor.PID
}

func (s *skynetActorImpl) Receive(c actor.Context) {
	switch msg := c.Message().(type) {
	case *skynetMsg:
		s.parent = c.Sender()
		if msg.size == 1 {
			c.Respond(&skynetResult{sum: msg.num})
			return
		}
		s.sum = 0
		s.received = 0
		newSize := msg.size / 10
		for j := int64(0); j < 10; j++ {
			childProps := actor.PropsFromProducer(func() actor.Actor {
				return &skynetActorImpl{system: s.system}
			})
			child := c.Spawn(childProps)
			c.Request(child, &skynetMsg{
				num:  msg.num + j*newSize,
				size: newSize,
			})
		}
	case *skynetResult:
		s.sum += msg.sum
		s.received++
		if s.received == 10 {
			if s.parent != nil {
				c.Send(s.parent, &skynetResult{sum: s.sum})
			}
		}
	}
}

func benchSkynet(system *actor.ActorSystem, warmup int, iterations int) stats {
	allDurations := make([]time.Duration, 0, warmup+iterations)

	for i := 0; i < warmup+iterations; i++ {
		rootProps := actor.PropsFromProducer(func() actor.Actor {
			return &skynetActorImpl{system: system}
		})

		start := time.Now()
		rootPID := system.Root.Spawn(rootProps)
		future := system.Root.RequestFuture(rootPID, &skynetMsg{num: 0, size: 1_000_000}, 30*time.Second)
		result, err := future.Result()
		elapsed := time.Since(start)

		if err != nil {
			fmt.Fprintf(os.Stderr, "skynet iteration %d error: %v\n", i, err)
		} else if r, ok := result.(*skynetResult); ok {
			expected := int64(499999500000)
			if r.sum != expected {
				fmt.Fprintf(os.Stderr, "skynet iteration %d: expected %d, got %d\n", i, expected, r.sum)
			}
		}

		_ = system.Root.StopFuture(rootPID).Wait()
		time.Sleep(10 * time.Millisecond)

		allDurations = append(allDurations, elapsed)
	}

	// Discard warmup iterations
	return computeStats(allDurations[warmup:])
}

// ---------------------------------------------------------------------------
// Benchmark: Cold Activation
// ---------------------------------------------------------------------------

func benchColdActivation(system *actor.ActorSystem, warmup int, iterations int) stats {
	allDurations := make([]time.Duration, 0, warmup+iterations)

	for i := 0; i < warmup+iterations; i++ {
		props := actor.PropsFromFunc(func(c actor.Context) {
			switch c.Message().(type) {
			case *warmPing:
				c.Respond(&warmPong{})
			}
		})

		start := time.Now()
		pid := system.Root.Spawn(props)
		future := system.Root.RequestFuture(pid, &warmPing{}, 5*time.Second)
		_, err := future.Result()
		elapsed := time.Since(start)

		if err != nil {
			fmt.Fprintf(os.Stderr, "cold activation iteration %d error: %v\n", i, err)
		}

		_ = system.Root.StopFuture(pid).Wait()

		allDurations = append(allDurations, elapsed)
	}

	// Discard warmup iterations
	return computeStats(allDurations[warmup:])
}

// ---------------------------------------------------------------------------
// Benchmark: Warm Message
// ---------------------------------------------------------------------------

func benchWarmMessage(system *actor.ActorSystem, warmup int, iterations int) stats {
	props := actor.PropsFromFunc(func(c actor.Context) {
		switch c.Message().(type) {
		case *warmPing:
			c.Respond(&warmPong{})
		}
	})

	pid := system.Root.Spawn(props)

	// Warm up the actor: send a few messages to ensure it is fully initialized
	for w := 0; w < 10; w++ {
		f := system.Root.RequestFuture(pid, &warmPing{}, 5*time.Second)
		f.Result()
	}

	allDurations := make([]time.Duration, 0, warmup+iterations)
	for i := 0; i < warmup+iterations; i++ {
		start := time.Now()
		future := system.Root.RequestFuture(pid, &warmPing{}, 5*time.Second)
		_, err := future.Result()
		elapsed := time.Since(start)

		if err != nil {
			fmt.Fprintf(os.Stderr, "warm message iteration %d error: %v\n", i, err)
		}

		allDurations = append(allDurations, elapsed)
	}

	_ = system.Root.StopFuture(pid).Wait()

	// Discard warmup iterations
	return computeStats(allDurations[warmup:])
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

func main() {
	system := actor.NewActorSystem()

	results := make(map[string]stats)

	fmt.Fprintln(os.Stderr, "Running: ping_pong_1k")
	results["ping_pong_1k"] = benchPingPong(system, 1_000, warmupStandard, iterPingPong1k)

	fmt.Fprintln(os.Stderr, "Running: ping_pong_10k")
	results["ping_pong_10k"] = benchPingPong(system, 10_000, warmupStandard, iterPingPong10k)

	fmt.Fprintln(os.Stderr, "Running: fan_out_10")
	results["fan_out_10"] = benchFanOut(system, 10, warmupStandard, iterFanOut10)

	fmt.Fprintln(os.Stderr, "Running: fan_out_100")
	results["fan_out_100"] = benchFanOut(system, 100, warmupStandard, iterFanOut100)

	fmt.Fprintln(os.Stderr, "Running: fan_out_1000")
	results["fan_out_1000"] = benchFanOut(system, 1000, warmupStandard, iterFanOut1000)

	fmt.Fprintln(os.Stderr, "Running: skynet_1m")
	results["skynet_1m"] = benchSkynet(system, warmupSkynet, iterSkynet)

	fmt.Fprintln(os.Stderr, "Running: cold_activation")
	results["cold_activation"] = benchColdActivation(system, warmupStandard, iterColdActivation)

	fmt.Fprintln(os.Stderr, "Running: warm_message")
	results["warm_message"] = benchWarmMessage(system, warmupStandard, iterWarmMessage)

	output := map[string]interface{}{
		"framework":  "protoactor-go",
		"benchmarks": results,
	}

	enc := json.NewEncoder(os.Stdout)
	enc.SetIndent("", "  ")
	if err := enc.Encode(output); err != nil {
		fmt.Fprintf(os.Stderr, "error encoding JSON: %v\n", err)
		os.Exit(1)
	}
}
