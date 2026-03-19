## Stage 1: Build with sbt + JDK 21
FROM eclipse-temurin:21-jdk AS builder

# Install sbt
RUN apt-get update && apt-get install -y curl gnupg && \
    echo "deb https://repo.scala-sbt.org/scalasbt/debian all main" > /etc/apt/sources.list.d/sbt.list && \
    curl -sL "https://keyserver.ubuntu.com/pks/lookup?op=get&search=0x2EE0EA64E40A89B84B2DF73499E82A75642AC823" | apt-key add && \
    apt-get update && apt-get install -y sbt && \
    rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Cache dependency resolution
COPY build.sbt .
COPY project/ project/
RUN sbt update

# Build fat jar
COPY src/ src/
RUN sbt assembly

## Stage 2: Slim JRE runner
FROM eclipse-temurin:21-jre

WORKDIR /app
COPY --from=builder /app/target/scala-2.13/akka-bench-assembly-0.1.0.jar app.jar

RUN mkdir -p /results

ENV BENCH_WARMUP=5
ENV BENCH_ITERATIONS=50

CMD ["sh", "-c", "java $JAVA_OPTS -jar app.jar > /results/akka.json 2>/dev/null"]
