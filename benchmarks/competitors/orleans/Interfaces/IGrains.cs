namespace Orleans.Bench.Interfaces;

/// <summary>
/// Ping-pong grain. Two instances alternate calling each other.
/// The "partner" grain key is passed so each side can look up the other via GrainFactory.
/// </summary>
public interface IPingPongGrain : IGrainWithIntegerKey
{
    /// <summary>
    /// Decrements remaining, then calls the partner grain's Pong.
    /// Returns 0 when remaining reaches 0.
    /// </summary>
    Task<int> Ping(int remaining, long partnerKey);
}

public interface IPongGrain : IGrainWithIntegerKey
{
    Task<int> Pong(int remaining, long partnerKey);
}

public interface IFanOutGrain : IGrainWithIntegerKey
{
    Task<long> Broadcast(int fanOut);
}

public interface IWorkerGrain : IGrainWithIntegerKey
{
    Task<int> Ack();
}

public interface ISkynetGrain : IGrainWithIntegerKey
{
    Task<long> Compute(long num, int size, int div);
}

public interface ICounterGrain : IGrainWithIntegerKey
{
    Task<int> Increment();
    Task<int> GetValue();
}
