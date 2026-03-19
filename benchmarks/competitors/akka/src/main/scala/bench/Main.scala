package bench

import akka.actor.ActorSystem
import com.google.gson.{Gson, GsonBuilder, JsonObject}
import scala.concurrent.Await
import scala.concurrent.duration._

object Main {

  private val gson: Gson = new GsonBuilder().setPrettyPrinting().create()

  def main(args: Array[String]): Unit = {
    val system = ActorSystem("bench")

    val warmup = 20
    val skynetWarmup = 5

    System.err.println(s"Akka benchmark: warmup=$warmup, skynetWarmup=$skynetWarmup")

    val benchmarks = new JsonObject()

    // --- Ping-pong 1K ---
    System.err.println("Running: ping_pong_1k")
    addStats(benchmarks, "ping_pong_1k",
      PingPong.run(system, roundTrips = 1000, warmup, iterations = 200))

    // --- Ping-pong 10K ---
    System.err.println("Running: ping_pong_10k")
    addStats(benchmarks, "ping_pong_10k",
      PingPong.run(system, roundTrips = 10000, warmup, iterations = 100))

    // --- Fan-out 1→10 ---
    System.err.println("Running: fan_out_10")
    addStats(benchmarks, "fan_out_10",
      FanOut.run(system, fanout = 10, warmup, iterations = 200))

    // --- Fan-out 1→100 ---
    System.err.println("Running: fan_out_100")
    addStats(benchmarks, "fan_out_100",
      FanOut.run(system, fanout = 100, warmup, iterations = 100))

    // --- Fan-out 1→1000 ---
    System.err.println("Running: fan_out_1000")
    addStats(benchmarks, "fan_out_1000",
      FanOut.run(system, fanout = 1000, warmup, iterations = 50))

    // --- Skynet 1M ---
    System.err.println("Running: skynet_1m")
    addStats(benchmarks, "skynet_1m",
      Skynet.run(system, warmup = skynetWarmup, iterations = 10))

    // --- Cold activation ---
    System.err.println("Running: cold_activation")
    addStats(benchmarks, "cold_activation",
      Activation.runCold(system, warmup, iterations = 1000))

    // --- Warm message ---
    System.err.println("Running: warm_message")
    addStats(benchmarks, "warm_message",
      Activation.runWarm(system, warmup, iterations = 1000))

    // Build output
    val root = new JsonObject()
    root.addProperty("framework", "akka")
    root.add("benchmarks", benchmarks)

    // JSON to stdout; progress/errors to stderr
    println(gson.toJson(root))

    Await.result(system.terminate(), 30.seconds)
  }

  private def addStats(parent: JsonObject, name: String, stats: BenchUtils.Stats): Unit = {
    val obj = new JsonObject()
    obj.addProperty("iterations", stats.iterations)
    obj.addProperty("mean_us", stats.meanUs)
    obj.addProperty("median_us", stats.medianUs)
    obj.addProperty("min_us", stats.minUs)
    obj.addProperty("max_us", stats.maxUs)
    obj.addProperty("p99_us", stats.p99Us)
    obj.addProperty("ips", stats.ips)
    parent.add(name, obj)
    System.err.println(s"  $name: mean=${stats.meanUs}us median=${stats.medianUs}us p99=${stats.p99Us}us ips=${stats.ips}")
  }
}
