FROM golang:1.26-alpine AS builder

RUN apk add --no-cache git

WORKDIR /app

COPY go.mod go.sum ./
RUN go mod download

COPY . .
RUN CGO_ENABLED=0 go build -o /bench ./main.go

FROM alpine:3.19

RUN mkdir -p /results

COPY --from=builder /bench /bench

CMD ["/bin/sh", "-c", "/bench > /results/protoactor.json 2>/dev/null"]
