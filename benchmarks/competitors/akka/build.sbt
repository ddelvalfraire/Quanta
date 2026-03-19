name := "akka-bench"
version := "0.1.0"
scalaVersion := "2.13.12"

libraryDependencies ++= Seq(
  "com.typesafe.akka" %% "akka-actor" % "2.8.8",
  "com.google.code.gson" % "gson" % "2.10.1"
)

// Silence Akka's SLF4J warnings — we don't need logging for benchmarks
libraryDependencies += "org.slf4j" % "slf4j-nop" % "2.0.9"

scalacOptions ++= Seq("-deprecation", "-feature", "-unchecked")

// Package as a fat jar for Docker
assembly / mainClass := Some("bench.Main")
assembly / assemblyMergeStrategy := {
  case PathList("reference.conf")            => MergeStrategy.concat
  case PathList("META-INF", "MANIFEST.MF")   => MergeStrategy.discard
  case PathList("META-INF", xs @ _*)         => MergeStrategy.discard
  case _                                     => MergeStrategy.first
}
