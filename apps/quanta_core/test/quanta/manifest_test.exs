defmodule Quanta.ManifestTest do
  use ExUnit.Case, async: true

  alias Quanta.Manifest

  @valid_yaml """
  version: "1"
  type: counter
  namespace: myapp
  state:
    kind: opaque
    max_size_bytes: 1048576
    snapshot_interval: 100
  lifecycle:
    idle_timeout_ms: 300000
    max_concurrent_messages: 1
  resources:
    fuel_limit: 1000000
    memory_limit_mb: 16
    max_timers: 100
  rate_limits:
    messages_per_second: 1000
    messages_per_second_type: 100000
  """

  describe "parse_yaml/1" do
    test "parses the §17.2 example" do
      assert {:ok, %Manifest{} = m} = Manifest.parse_yaml(@valid_yaml)
      assert m.version == "1"
      assert m.type == "counter"
      assert m.namespace == "myapp"
      assert m.state.kind == :opaque
      assert m.state.max_size_bytes == 1_048_576
      assert m.state.snapshot_interval == 100
      assert m.lifecycle.idle_timeout_ms == 300_000
      assert m.lifecycle.max_concurrent_messages == 1
      assert m.resources.fuel_limit == 1_000_000
      assert m.resources.memory_limit_mb == 16
      assert m.resources.max_timers == 100
      assert m.rate_limits.messages_per_second == 1_000
      assert m.rate_limits.messages_per_second_type == 100_000
    end

    test "applies defaults for all optional fields" do
      yaml = """
      version: "1"
      type: counter
      namespace: myapp
      """

      assert {:ok, %Manifest{} = m} = Manifest.parse_yaml(yaml)
      assert m.state.kind == :opaque
      assert m.state.version == 1
      assert m.state.max_size_bytes == 1_048_576
      assert m.state.snapshot_interval == 100
      assert m.lifecycle.idle_timeout_ms == 300_000
      assert m.lifecycle.idle_no_subscribers_timeout_ms == 30_000
      assert m.lifecycle.max_concurrent_messages == 1
      assert m.lifecycle.inter_actor_timeout_ms == 30_000
      assert m.lifecycle.http_timeout_ms == 5_000
      assert m.resources.fuel_limit == 1_000_000
      assert m.resources.memory_limit_mb == 16
      assert m.resources.max_timers == 100
      assert m.rate_limits.messages_per_second == 1_000
      assert m.rate_limits.messages_per_second_type == 100_000
    end

    test "parses all valid state.kind values" do
      for kind <- ["opaque", "crdt:text", "crdt:map", "crdt:list", "crdt:tree", "crdt:counter"] do
        yaml = """
        version: "1"
        type: counter
        namespace: myapp
        state:
          kind: #{kind}
        """

        assert {:ok, %Manifest{}} = Manifest.parse_yaml(yaml),
               "expected #{kind} to be valid"
      end
    end

    test "parses schematized and authoritative kinds" do
      yaml = """
      version: "1"
      type: counter
      namespace: myapp
      state:
        kind: "schematized:v1.schema"
      """

      assert {:ok, %Manifest{state: state}} = Manifest.parse_yaml(yaml)
      assert state.kind == {:schematized, "v1.schema"}

      yaml2 = """
      version: "1"
      type: counter
      namespace: myapp
      state:
        kind: authoritative
      """

      assert {:ok, %Manifest{state: state2}} = Manifest.parse_yaml(yaml2)
      assert state2.kind == {:authoritative, nil}
    end

    test "rejects invalid YAML" do
      assert {:error, ["YAML parse error:" <> _]} = Manifest.parse_yaml("{invalid: [")
    end

    test "rejects non-mapping YAML" do
      assert {:error, ["manifest must be a YAML mapping"]} = Manifest.parse_yaml("- a list")
    end

    test "rejects YAML exceeding size limit" do
      huge = String.duplicate("x", 65_537)
      assert {:error, [msg]} = Manifest.parse_yaml(huge)
      assert msg =~ "exceeds maximum size"
    end

    test "preserves explicit zero values for validation" do
      yaml = """
      version: "1"
      type: counter
      namespace: myapp
      lifecycle:
        idle_timeout_ms: 0
      """

      assert {:error, errors} = Manifest.parse_yaml(yaml)
      assert Enum.any?(errors, &String.contains?(&1, "lifecycle.idle_timeout_ms"))
    end
  end

  describe "validate/1 — version" do
    test "rejects missing version" do
      m = %Manifest{version: nil, type: "counter", namespace: "myapp"}
      assert {:error, errors} = Manifest.validate(m)
      assert "version is required" in errors
    end

    test "rejects wrong version" do
      m = %Manifest{version: "2", type: "counter", namespace: "myapp"}
      assert {:error, errors} = Manifest.validate(m)
      assert Enum.any?(errors, &String.contains?(&1, "version must be \"1\""))
    end
  end

  describe "validate/1 — type and namespace" do
    test "rejects invalid type" do
      m = %Manifest{version: "1", type: "has.dots", namespace: "myapp"}
      assert {:error, errors} = Manifest.validate(m)
      assert Enum.any?(errors, &String.contains?(&1, "type must match"))
    end

    test "rejects missing namespace" do
      m = %Manifest{version: "1", type: "counter", namespace: nil}
      assert {:error, errors} = Manifest.validate(m)
      assert "namespace is required" in errors
    end

    test "rejects too-long type" do
      long = String.duplicate("a", 64)
      m = %Manifest{version: "1", type: long, namespace: "myapp"}
      assert {:error, errors} = Manifest.validate(m)
      assert Enum.any?(errors, &String.contains?(&1, "type must match"))
    end
  end

  describe "validate/1 — state" do
    test "rejects invalid state.kind string" do
      yaml = """
      version: "1"
      type: counter
      namespace: myapp
      state:
        kind: invalid_kind
      """

      assert {:error, errors} = Manifest.parse_yaml(yaml)
      assert Enum.any?(errors, &String.contains?(&1, "invalid state.kind"))
    end

    test "rejects state.version out of range" do
      m = %Manifest{
        version: "1",
        type: "counter",
        namespace: "myapp",
        state: %Manifest.State{version: 0}
      }

      assert {:error, errors} = Manifest.validate(m)
      assert Enum.any?(errors, &String.contains?(&1, "state.version must be between"))
    end

    test "rejects state.version above 65535" do
      m = %Manifest{
        version: "1",
        type: "counter",
        namespace: "myapp",
        state: %Manifest.State{version: 65_536}
      }

      assert {:error, errors} = Manifest.validate(m)
      assert Enum.any?(errors, &String.contains?(&1, "state.version must be between"))
    end

    test "rejects state.max_size_bytes above 8MB" do
      m = %Manifest{
        version: "1",
        type: "counter",
        namespace: "myapp",
        state: %Manifest.State{max_size_bytes: 8_388_609}
      }

      assert {:error, errors} = Manifest.validate(m)
      assert Enum.any?(errors, &String.contains?(&1, "state.max_size_bytes must be between"))
    end
  end

  describe "validate/1 — returns all errors" do
    test "accumulates multiple validation errors" do
      m = %Manifest{
        version: "2",
        type: nil,
        namespace: "has spaces",
        state: %Manifest.State{version: 0, max_size_bytes: -1}
      }

      assert {:error, errors} = Manifest.validate(m)
      assert length(errors) >= 4
    end
  end

  describe "validate/1 — lifecycle, resources, rate_limits" do
    test "rejects zero or negative lifecycle values" do
      m = %Manifest{
        version: "1",
        type: "counter",
        namespace: "myapp",
        lifecycle: %Manifest.Lifecycle{idle_timeout_ms: 0}
      }

      assert {:error, errors} = Manifest.validate(m)
      assert Enum.any?(errors, &String.contains?(&1, "lifecycle.idle_timeout_ms"))
    end

    test "rejects negative resource values" do
      m = %Manifest{
        version: "1",
        type: "counter",
        namespace: "myapp",
        resources: %Manifest.Resources{fuel_limit: -1}
      }

      assert {:error, errors} = Manifest.validate(m)
      assert Enum.any?(errors, &String.contains?(&1, "resources.fuel_limit"))
    end

    test "rejects non-integer rate limits" do
      m = %Manifest{
        version: "1",
        type: "counter",
        namespace: "myapp",
        rate_limits: %Manifest.RateLimits{messages_per_second: "fast"}
      }

      assert {:error, errors} = Manifest.validate(m)
      assert Enum.any?(errors, &String.contains?(&1, "rate_limits.messages_per_second"))
    end
  end

  describe "validate_update/2" do
    test "allows updating mutable fields" do
      old = %Manifest{version: "1", type: "counter", namespace: "myapp"}
      new = %Manifest{version: "1", type: "counter", namespace: "myapp", state: %Manifest.State{version: 2}}
      assert :ok = Manifest.validate_update(old, new)
    end

    test "rejects namespace change" do
      old = %Manifest{version: "1", type: "counter", namespace: "ns1"}
      new = %Manifest{version: "1", type: "counter", namespace: "ns2"}
      assert {:error, msg} = Manifest.validate_update(old, new)
      assert msg =~ "namespace is immutable"
    end

    test "rejects type change" do
      old = %Manifest{version: "1", type: "counter", namespace: "myapp"}
      new = %Manifest{version: "1", type: "timer", namespace: "myapp"}
      assert {:error, msg} = Manifest.validate_update(old, new)
      assert msg =~ "type is immutable"
    end

    test "rejects state.kind change" do
      old = %Manifest{version: "1", type: "counter", namespace: "myapp", state: %Manifest.State{kind: :opaque}}
      new = %Manifest{version: "1", type: "counter", namespace: "myapp", state: %Manifest.State{kind: {:crdt, :map}}}
      assert {:error, msg} = Manifest.validate_update(old, new)
      assert msg =~ "state.kind is immutable"
    end
  end
end
