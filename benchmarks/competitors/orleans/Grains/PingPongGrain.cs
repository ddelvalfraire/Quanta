using Orleans.Bench.Interfaces;
using Orleans.Concurrency;

namespace Orleans.Bench.Grains;

/// <summary>
/// Grain A in the ping-pong pair. Calls IPongGrain on the partner side.
/// Must be [Reentrant] because grain B calls back into grain A while A's
/// original call is still awaiting B's response.
/// </summary>
[Reentrant]
public class PingPongGrain : Grain, IPingPongGrain
{
    public async Task<int> Ping(int remaining, long partnerKey)
    {
        if (remaining <= 0)
            return 0;

        var partner = GrainFactory.GetGrain<IPongGrain>(partnerKey);
        return await partner.Pong(remaining - 1, this.GetPrimaryKeyLong());
    }
}

/// <summary>
/// Grain B in the ping-pong pair. Calls IPingPongGrain on the partner side.
/// Must be [Reentrant] for the same reason as PingPongGrain.
/// </summary>
[Reentrant]
public class PongGrain : Grain, IPongGrain
{
    public async Task<int> Pong(int remaining, long partnerKey)
    {
        if (remaining <= 0)
            return 0;

        var partner = GrainFactory.GetGrain<IPingPongGrain>(partnerKey);
        return await partner.Ping(remaining - 1, this.GetPrimaryKeyLong());
    }
}
