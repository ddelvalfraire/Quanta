package bench

import akka.actor.{Actor, ActorRef, ActorSystem, Props}
import java.util.concurrent.CountDownLatch

object PingPong {

  // Messages
  case object Ping
  case object Pong
  case class Start(roundTrips: Int, peer: ActorRef, latch: CountDownLatch)

  class PingActor extends Actor {
    private var remaining = 0
    private var peer: ActorRef = _
    private var latch: CountDownLatch = _

    def receive: Receive = {
      case Start(n, p, l) =>
        remaining = n
        peer = p
        latch = l
        peer ! Ping

      case Pong =>
        remaining -= 1
        if (remaining > 0) {
          peer ! Ping
        } else {
          latch.countDown()
        }
    }
  }

  class PongActor extends Actor {
    def receive: Receive = {
      case Ping => sender() ! Pong
    }
  }

  def run(system: ActorSystem, roundTrips: Int, warmup: Int, iterations: Int): BenchUtils.Stats = {
    val times = BenchUtils.measure(warmup, iterations) {
      val latch = new CountDownLatch(1)
      val pong = system.actorOf(Props[PongActor]())
      val ping = system.actorOf(Props[PingActor]())
      ping ! Start(roundTrips, pong, latch)
      latch.await()
      // Actors are cleaned up when the ActorSystem shuts down.
      // Stopping inside the timed block would inflate measurements.
    }
    BenchUtils.computeStats(times)
  }
}
