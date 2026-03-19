package bench

import akka.actor.{Actor, ActorSystem, Props}
import akka.pattern.ask
import akka.util.Timeout
import scala.concurrent.Await
import scala.concurrent.duration._

object Activation {

  case object Ping
  case object Pong

  class EchoActor extends Actor {
    def receive: Receive = {
      case Ping => sender() ! Pong
    }
  }

  implicit val timeout: Timeout = Timeout(10.seconds)

  /** Cold activation: measure from actorOf() through first ask+reply. */
  def runCold(system: ActorSystem, warmup: Int, iterations: Int): BenchUtils.Stats = {
    val times = BenchUtils.measure(warmup, iterations) {
      val actor = system.actorOf(Props[EchoActor]())
      val future = actor ? Ping
      Await.result(future, 10.seconds)
      system.stop(actor)
    }
    BenchUtils.computeStats(times)
  }

  /** Warm message: pre-spawn actor, measure only the ask+reply latency. */
  def runWarm(system: ActorSystem, warmup: Int, iterations: Int): BenchUtils.Stats = {
    val actor = system.actorOf(Props[EchoActor]())
    // Pre-warm: send one message to ensure actor is initialized
    Await.result(actor ? Ping, 10.seconds)

    val times = BenchUtils.measure(warmup, iterations) {
      val future = actor ? Ping
      Await.result(future, 10.seconds)
    }

    system.stop(actor)
    BenchUtils.computeStats(times)
  }
}
