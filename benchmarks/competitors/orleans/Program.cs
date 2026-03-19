using Microsoft.Extensions.DependencyInjection;
using Microsoft.Extensions.Hosting;
using Microsoft.Extensions.Logging;
using Orleans.Bench;

// Build an in-process Orleans silo with no external dependencies.
var builder = Host.CreateDefaultBuilder(args)
    .UseOrleans(silo =>
    {
        silo.UseLocalhostClustering();
        silo.AddMemoryGrainStorageAsDefault();
    })
    .ConfigureLogging(logging =>
    {
        // Suppress Orleans info/debug logging — we only want benchmark JSON on stdout.
        logging.SetMinimumLevel(Microsoft.Extensions.Logging.LogLevel.Warning);
    });

using var host = builder.Build();
await host.StartAsync();

Console.Error.WriteLine("[orleans-bench] Silo started. Running benchmarks...");

var grainFactory = host.Services.GetRequiredService<IGrainFactory>();
await BenchRunner.RunAll(grainFactory, Console.Out);

Console.Error.WriteLine("[orleans-bench] Done.");
await host.StopAsync();
