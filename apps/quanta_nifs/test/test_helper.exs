nats_available? =
  case :gen_tcp.connect(~c"localhost", 4222, [], 1_000) do
    {:ok, socket} ->
      :gen_tcp.close(socket)
      true

    {:error, _} ->
      false
  end

excludes = [:multi_node, :integration, :chaos, :load]
excludes = if nats_available?, do: excludes, else: [:nats | excludes]

ExUnit.start(exclude: excludes)
