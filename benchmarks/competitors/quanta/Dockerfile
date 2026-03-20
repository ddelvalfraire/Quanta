FROM elixir:1.19-alpine AS builder
RUN apk add --no-cache build-base git rust cargo
WORKDIR /app
# Copy the entire Quanta project
COPY . .
RUN mix local.hex --force && mix local.rebar --force
ENV MIX_ENV=dev
RUN mix deps.get && mix compile
RUN mkdir -p /results

# Set required env vars
ENV QUANTA_WASM_HMAC_KEY=0000000000000000000000000000000000000000000000000000000000000000
ENV QUANTA_SECRET_KEY_BASE=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
ENV QUANTA_API_KEYS=bench_key
ENV QUANTA_LOG_LEVEL=error

CMD ["sh", "-c", "mix bench.competitor > /results/quanta.json 2>/dev/null"]
