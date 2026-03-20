package bench

import akka.actor.{Actor, ActorRef, ActorSystem, Props}
import akka.pattern.ask
import akka.util.Timeout
import java.util.concurrent.CountDownLatch
import scala.concurrent.Await
import scala.concurrent.duration._

object Activation {

  case class Ping(replyTo: ActorRef, latch: CountDownLatch)
  case object AskPing

  class EchoActor extends Actor {
    def receive: Receive = {
      case Ping(replyTo, latch) =>
        replyTo ! "pong"
        latch.countDown()
      case AskPing =>
        sender() ! "pong"
    }
  }

  /** Cold activation: measure from actorOf() through first tell+reply via latch. */
  def runCold(system: ActorSystem, warmup: Int, iterations: Int): BenchUtils.Stats = {
    val times = BenchUtils.measure(warmup, iterations) {
      val latch = new CountDownLatch(1)
      val actor = system.actorOf(Props[EchoActor]())
      val collector = system.actorOf(Props(new CollectorActor(latch)))
      actor ! Ping(collector, latch)
      latch.await()
      // Actors are cleaned up when the ActorSystem shuts down.
      // Stopping inside the timed block would inflate measurements.
    }

    BenchUtils.computeStats(times)
  }

  /** Helper collector used only in cold activation. */
  class CollectorActor(latch: CountDownLatch) extends Actor {
    def receive: Receive = {
      case _ => latch.countDown()
    }
  }

  /** Warm message: pre-spawn actor, measure only the ask+reply latency. */
  def runWarm(system: ActorSystem, warmup: Int, iterations: Int): BenchUtils.Stats = {
    implicit val timeout: Timeout = Timeout(5.seconds)
    val actor = system.actorOf(Props[EchoActor]())

    // Pre-warm the actor
    for (_ <- 1 to 10) {
      Await.result(actor ? AskPing, 5.seconds)
    }

    val times = BenchUtils.measure(warmup, iterations) {
      Await.result(actor ? AskPing, 5.seconds)
    }

    system.stop(actor)
    BenchUtils.computeStats(times)
  }
}
