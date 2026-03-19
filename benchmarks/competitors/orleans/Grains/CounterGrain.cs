using Orleans.Bench.Interfaces;

namespace Orleans.Bench.Grains;

public class CounterGrain : Grain, ICounterGrain
{
    private int _value;

    public Task<int> Increment()
    {
        _value++;
        return Task.FromResult(_value);
    }

    public Task<int> GetValue()
    {
        return Task.FromResult(_value);
    }
}
