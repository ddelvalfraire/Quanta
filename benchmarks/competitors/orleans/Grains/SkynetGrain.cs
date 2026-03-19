using Orleans.Bench.Interfaces;
using Orleans.Concurrency;

namespace Orleans.Bench.Grains;

/// <summary>
/// Skynet benchmark grain. Creates a tree of grains that sum leaf values.
/// Each grain is keyed by a unique ID derived from its position in the tree.
///
/// Key scheme: root is 1, children of node N are (N*div + 1) through (N*div + div).
/// This guarantees unique keys and no collisions with the parent.
/// The leaf value for a grain is computed from its tree position.
/// </summary>
[Reentrant]
public class SkynetGrain : Grain, ISkynetGrain
{
    /// <param name="num">The leaf value for this node (only used when size == 1).</param>
    /// <param name="size">Number of leaves in this subtree.</param>
    /// <param name="div">Branching factor (always 10 for skynet).</param>
    public async Task<long> Compute(long num, int size, int div)
    {
        if (size <= 1)
            return num;

        var childSize = size / div;
        var tasks = new Task<long>[div];

        for (int i = 0; i < div; i++)
        {
            var childNum = num + (long)i * childSize;
            // Use a unique key: parent's key * div + (i + 1) to avoid collision with parent.
            long childKey = this.GetPrimaryKeyLong() * div + i + 1;
            var child = GrainFactory.GetGrain<ISkynetGrain>(childKey);
            tasks[i] = child.Compute(childNum, childSize, div);
        }

        var results = await Task.WhenAll(tasks);
        long sum = 0;
        for (int i = 0; i < results.Length; i++)
            sum += results[i];

        return sum;
    }
}
