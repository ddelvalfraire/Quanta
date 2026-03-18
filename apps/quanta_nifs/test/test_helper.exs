nats_available? =
  case :gen_tcp.connect(~c"localhost", 4222, [], 1_000) do
    {:ok, socket} ->
      :gen_tcp.close(socket)
      true

    {:error, _} ->
      false
  end

unless nats_available? do
  ExUnit.configure(exclude: [:nats])
end

ExUnit.start()
