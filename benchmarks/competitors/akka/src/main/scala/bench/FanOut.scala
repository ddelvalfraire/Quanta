package bench

import akka.actor.{Actor, ActorRef, ActorSystem, Props}
import java.util.concurrent.CountDownLatch

object FanOut {

  case class Broadcast(msg: String)
  case object Ack
  case class Go(workers: Array[ActorRef], latch: CountDownLatch)

  class Coordinator extends Actor {
    private var expected = 0
    private var received = 0
    private var latch: CountDownLatch = _

    def receive: Receive = {
      case Go(workers, l) =>
        latch = l
        expected = workers.length
        received = 0
        val msg = Broadcast("work")
        var i = 0
        while (i < workers.length) {
          workers(i) ! msg
          i += 1
        }

      case Ack =>
        received += 1
        if (received >= expected) {
          latch.countDown()
        }
    }
  }

  class Worker extends Actor {
    def receive: Receive = {
      case Broadcast(_) =>
        sender() ! Ack
    }
  }

  def run(system: ActorSystem, fanout: Int, warmup: Int, iterations: Int): BenchUtils.Stats = {
    val times = BenchUtils.measure(warmup, iterations) {
      val latch = new CountDownLatch(1)
      val workers = Array.tabulate(fanout)(_ => system.actorOf(Props[Worker]()))
      val coordinator = system.actorOf(Props[Coordinator]())
      coordinator ! Go(workers, latch)
      latch.await()
      // Actors are cleaned up when the ActorSystem shuts down.
      // Stopping inside the timed block would inflate measurements.
    }
    BenchUtils.computeStats(times)
  }
}
