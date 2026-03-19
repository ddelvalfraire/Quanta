package bench

import akka.actor.{Actor, ActorRef, ActorSystem, Props}
import java.util.concurrent.CountDownLatch

object Skynet {

  case class Start(parent: ActorRef, num: Long, size: Long)
  case class Result(value: Long)

  class SkynetActor extends Actor {
    private var parent: ActorRef = _
    private var sum: Long = 0L
    private var remaining: Int = 0

    def receive: Receive = {
      case Start(p, num, size) =>
        parent = p
        if (size == 1) {
          parent ! Result(num)
          context.stop(self)
        } else {
          remaining = 10
          sum = 0L
          val childSize = size / 10
          var i = 0
          while (i < 10) {
            val child = context.actorOf(Props[SkynetActor]())
            child ! Start(self, num + i * childSize, childSize)
            i += 1
          }
        }

      case Result(value) =>
        sum += value
        remaining -= 1
        if (remaining == 0) {
          parent ! Result(sum)
          context.stop(self)
        }
    }
  }

  /** Collector sits at the root and signals completion via a latch. */
  class Collector(latch: CountDownLatch, resultHolder: Array[Long]) extends Actor {
    def receive: Receive = {
      case Result(value) =>
        resultHolder(0) = value
        latch.countDown()
    }
  }

  def run(system: ActorSystem, warmup: Int, iterations: Int): BenchUtils.Stats = {
    val resultHolder = new Array[Long](1)

    val times = BenchUtils.measure(warmup, iterations) {
      val latch = new CountDownLatch(1)
      val collector = system.actorOf(Props(new Collector(latch, resultHolder)))
      val root = system.actorOf(Props[SkynetActor]())
      root ! Start(collector, 0L, 1000000L)
      latch.await()
      // Collector is cleaned up when the ActorSystem shuts down.
      // Stopping inside the timed block would inflate measurements.
    }

    // Sanity check: sum of 0..999999 = 499999500000
    assert(resultHolder(0) == 499999500000L,
      s"Skynet result mismatch: expected 499999500000, got ${resultHolder(0)}")

    BenchUtils.computeStats(times)
  }
}
