using Orleans.Bench.Interfaces;

namespace Orleans.Bench.Grains;

public class FanOutGrain : Grain, IFanOutGrain
{
    public async Task<long> Broadcast(int fanOut)
    {
        var tasks = new Task<int>[fanOut];
        for (int i = 0; i < fanOut; i++)
        {
            var worker = GrainFactory.GetGrain<IWorkerGrain>(i);
            tasks[i] = worker.Ack();
        }

        var results = await Task.WhenAll(tasks);
        return results.Sum();
    }
}

public class WorkerGrain : Grain, IWorkerGrain
{
    public Task<int> Ack()
    {
        return Task.FromResult(1);
    }
}
