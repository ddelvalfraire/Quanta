using System.Diagnostics;
using System.Text.Json;
using System.Text.Json.Serialization;
using Orleans.Bench.Interfaces;

namespace Orleans.Bench;

public static class BenchRunner
{
    private static readonly double TicksPerMicrosecond = Stopwatch.Frequency / 1_000_000.0;

    public static async Task RunAll(IGrainFactory grainFactory, TextWriter output)
    {
        var results = new Dictionary<string, BenchResult>();

        // --- Ping-pong 1K ---
        results["ping_pong_1k"] = await RunBench("ping_pong_1k", 50, async () =>
        {
            var a = grainFactory.GetGrain<IPingPongGrain>(1);
            await a.Ping(1_000, /*partnerKey=*/ 2);
        });

        // --- Ping-pong 10K ---
        results["ping_pong_10k"] = await RunBench("ping_pong_10k", 20, async () =>
        {
            var a = grainFactory.GetGrain<IPingPongGrain>(3);
            await a.Ping(10_000, /*partnerKey=*/ 4);
        });

        // --- Fan-out 10 ---
        results["fan_out_10"] = await RunBench("fan_out_10", 100, async () =>
        {
            var g = grainFactory.GetGrain<IFanOutGrain>(10);
            await g.Broadcast(10);
        });

        // --- Fan-out 100 ---
        results["fan_out_100"] = await RunBench("fan_out_100", 50, async () =>
        {
            var g = grainFactory.GetGrain<IFanOutGrain>(100);
            await g.Broadcast(100);
        });

        // --- Fan-out 1000 ---
        results["fan_out_1000"] = await RunBench("fan_out_1000", 20, async () =>
        {
            var g = grainFactory.GetGrain<IFanOutGrain>(1000);
            await g.Broadcast(1_000);
        });

        // --- Skynet 100K ---
        results["skynet_100k"] = await RunBench("skynet_100k", 3, async () =>
        {
            var root = grainFactory.GetGrain<ISkynetGrain>(1);
            var result = await root.Compute(0, 100_000, 10);
            // Expected: sum of 0..99999 = 4999950000
            if (result != 4_999_950_000L)
                Console.Error.WriteLine($"[WARN] skynet_100k: expected 4999950000, got {result}");
        });

        // --- Cold activation ---
        results["cold_activation"] = await RunColdActivation(grainFactory, 200);

        // --- Warm message ---
        results["warm_message"] = await RunWarmMessage(grainFactory, 1000);

        var envelope = new BenchEnvelope
        {
            Framework = "orleans",
            Benchmarks = results
        };

        var json = JsonSerializer.Serialize(envelope, SerializerContext.Default.BenchEnvelope);
        await output.WriteLineAsync(json);
    }

    private static async Task<BenchResult> RunBench(string name, int iterations, Func<Task> action)
    {
        Console.Error.WriteLine($"[bench] {name}: warming up...");
        // Warmup
        await action();

        Console.Error.WriteLine($"[bench] {name}: running {iterations} iterations...");
        var timings = new double[iterations];
        var sw = new Stopwatch();

        for (int i = 0; i < iterations; i++)
        {
            sw.Restart();
            await action();
            sw.Stop();
            timings[i] = sw.ElapsedTicks / TicksPerMicrosecond;
        }

        return ComputeStats(iterations, timings);
    }

    private static async Task<BenchResult> RunColdActivation(IGrainFactory grainFactory, int iterations)
    {
        const string name = "cold_activation";
        Console.Error.WriteLine($"[bench] {name}: running {iterations} iterations...");

        var timings = new double[iterations];
        var sw = new Stopwatch();
        var rng = new Random(42);

        for (int i = 0; i < iterations; i++)
        {
            // Use a unique random key each time to force a fresh grain activation
            long key = rng.NextInt64(1_000_000_000L, long.MaxValue);
            sw.Restart();
            var grain = grainFactory.GetGrain<ICounterGrain>(key);
            await grain.Increment();
            sw.Stop();
            timings[i] = sw.ElapsedTicks / TicksPerMicrosecond;
        }

        return ComputeStats(iterations, timings);
    }

    private static async Task<BenchResult> RunWarmMessage(IGrainFactory grainFactory, int iterations)
    {
        const string name = "warm_message";
        Console.Error.WriteLine($"[bench] {name}: warming up...");

        // Pre-activate the grain
        var grain = grainFactory.GetGrain<ICounterGrain>(999_999);
        await grain.Increment();

        Console.Error.WriteLine($"[bench] {name}: running {iterations} iterations...");
        var timings = new double[iterations];
        var sw = new Stopwatch();

        for (int i = 0; i < iterations; i++)
        {
            sw.Restart();
            await grain.Increment();
            sw.Stop();
            timings[i] = sw.ElapsedTicks / TicksPerMicrosecond;
        }

        return ComputeStats(iterations, timings);
    }

    private static BenchResult ComputeStats(int iterations, double[] timings)
    {
        Array.Sort(timings);

        double sum = 0;
        for (int i = 0; i < timings.Length; i++)
            sum += timings[i];

        double mean = sum / timings.Length;
        double median = timings.Length % 2 == 0
            ? (timings[timings.Length / 2 - 1] + timings[timings.Length / 2]) / 2.0
            : timings[timings.Length / 2];
        double min = timings[0];
        double max = timings[^1];

        int p99Index = (int)Math.Ceiling(timings.Length * 0.99) - 1;
        if (p99Index < 0) p99Index = 0;
        if (p99Index >= timings.Length) p99Index = timings.Length - 1;
        double p99 = timings[p99Index];

        double ips = mean > 0 ? 1_000_000.0 / mean : 0;

        return new BenchResult
        {
            Iterations = iterations,
            MeanUs = Math.Round(mean, 2),
            MedianUs = Math.Round(median, 2),
            MinUs = Math.Round(min, 2),
            MaxUs = Math.Round(max, 2),
            P99Us = Math.Round(p99, 2),
            Ips = Math.Round(ips, 2)
        };
    }
}

public class BenchEnvelope
{
    [JsonPropertyName("framework")]
    public string Framework { get; set; } = "";

    [JsonPropertyName("benchmarks")]
    public Dictionary<string, BenchResult> Benchmarks { get; set; } = new();
}

public class BenchResult
{
    [JsonPropertyName("iterations")]
    public int Iterations { get; set; }

    [JsonPropertyName("mean_us")]
    public double MeanUs { get; set; }

    [JsonPropertyName("median_us")]
    public double MedianUs { get; set; }

    [JsonPropertyName("min_us")]
    public double MinUs { get; set; }

    [JsonPropertyName("max_us")]
    public double MaxUs { get; set; }

    [JsonPropertyName("p99_us")]
    public double P99Us { get; set; }

    [JsonPropertyName("ips")]
    public double Ips { get; set; }
}

[JsonSerializable(typeof(BenchEnvelope))]
[JsonSerializable(typeof(BenchResult))]
[JsonSerializable(typeof(Dictionary<string, BenchResult>))]
internal partial class SerializerContext : JsonSerializerContext
{
}
