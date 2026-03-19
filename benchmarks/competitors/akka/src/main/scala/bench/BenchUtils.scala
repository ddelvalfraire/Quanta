package bench

object BenchUtils {

  case class Stats(
    iterations: Int,
    meanUs: Double,
    medianUs: Double,
    minUs: Double,
    maxUs: Double,
    p99Us: Double,
    ips: Double
  )

  /** Run `body` for `warmup` iterations (discarded), then `iterations` measured runs.
    * Returns wall-clock nanos for each measured iteration.
    */
  def measure(warmup: Int, iterations: Int)(body: => Unit): Array[Long] = {
    // Warmup
    var i = 0
    while (i < warmup) {
      body
      i += 1
    }

    // Measured
    val times = new Array[Long](iterations)
    i = 0
    while (i < iterations) {
      val t0 = System.nanoTime()
      body
      val t1 = System.nanoTime()
      times(i) = t1 - t0
      i += 1
    }
    times
  }

  def computeStats(timesNanos: Array[Long]): Stats = {
    val sorted = timesNanos.sorted
    val n = sorted.length

    val minNs = sorted(0).toDouble
    val maxNs = sorted(n - 1).toDouble
    val meanNs = sorted.map(_.toDouble).sum / n
    val medianNs =
      if (n % 2 == 1) sorted(n / 2).toDouble
      else (sorted(n / 2 - 1) + sorted(n / 2)).toDouble / 2.0
    val p99Idx = Math.min(((n - 1) * 0.99).ceil.toInt, n - 1)
    val p99Ns = sorted(p99Idx).toDouble

    val meanUs = meanNs / 1000.0
    val ips = if (meanUs > 0) 1_000_000.0 / meanUs else 0.0

    Stats(
      iterations = n,
      meanUs = round2(meanUs),
      medianUs = round2(medianNs / 1000.0),
      minUs = round2(minNs / 1000.0),
      maxUs = round2(maxNs / 1000.0),
      p99Us = round2(p99Ns / 1000.0),
      ips = round2(ips)
    )
  }

  private def round2(v: Double): Double =
    Math.round(v * 100.0) / 100.0
}
