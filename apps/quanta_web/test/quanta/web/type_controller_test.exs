defmodule Quanta.Web.TypeControllerTest do
  use Quanta.Web.ConnCase, async: false

  alias Quanta.Actor.ManifestRegistry

  @valid_manifest """
  version: "1"
  namespace: test
  type: greeter
  """

  defp auth(conn, key \\ @admin_key) do
    put_req_header(conn, "authorization", "Bearer #{key}")
  end

  describe "GET /api/v1/types/:ns" do
    test "lists types in namespace", %{conn: conn} do
      conn =
        conn
        |> auth(@ro_key)
        |> get("/api/v1/types/test")

      types = json_response(conn, 200)
      assert is_list(types)
      assert Enum.any?(types, &(&1["type"] == "counter"))
    end

    test "returns empty list for unknown namespace", %{conn: conn} do
      # Use a key scoped to this namespace
      Application.put_env(:quanta_web, :api_keys, [
        "qk_admin_test_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "qk_rw_test_bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
        "qk_ro_test_cccccccccccccccccccccccccccccccc",
        "qk_ro_nonexistent_dddddddddddddddddddddddddddddddd"
      ])

      conn =
        conn
        |> put_req_header(
          "authorization",
          "Bearer qk_ro_nonexistent_dddddddddddddddddddddddddddddddd"
        )
        |> get("/api/v1/types/nonexistent")

      assert json_response(conn, 200) == []
    end
  end

  describe "POST /api/v1/types/:ns/:type/deploy" do
    test "deploys manifest from upload", %{conn: conn} do
      upload = %Plug.Upload{
        path: write_temp(@valid_manifest),
        filename: "manifest.yaml",
        content_type: "application/x-yaml"
      }

      conn =
        conn
        |> auth()
        |> post("/api/v1/types/test/greeter/deploy", %{manifest: upload})

      body = json_response(conn, 200)
      assert body["namespace"] == "test"
      assert body["type"] == "greeter"
      assert body["version"] == "1"

      # Verify actually stored
      assert {:ok, _} = ManifestRegistry.get("test", "greeter")
    end

    test "returns 501 if wasm part is present", %{conn: conn} do
      upload = %Plug.Upload{
        path: write_temp(@valid_manifest),
        filename: "manifest.yaml",
        content_type: "application/x-yaml"
      }

      wasm_upload = %Plug.Upload{
        path: write_temp("fake wasm"),
        filename: "module.wasm",
        content_type: "application/wasm"
      }

      conn =
        conn
        |> auth()
        |> post("/api/v1/types/test/greeter/deploy", %{manifest: upload, wasm: wasm_upload})

      assert json_response(conn, 501)["error"] == "wasm_not_available"
    end

    test "returns 422 for invalid manifest YAML", %{conn: conn} do
      upload = %Plug.Upload{
        path: write_temp("not: valid: manifest:"),
        filename: "manifest.yaml",
        content_type: "application/x-yaml"
      }

      conn =
        conn
        |> auth()
        |> post("/api/v1/types/test/broken/deploy", %{manifest: upload})

      assert conn.status == 422
    end

    test "returns 422 when namespace mismatches URL", %{conn: conn} do
      manifest = """
      version: "1"
      namespace: other
      type: greeter
      """

      upload = %Plug.Upload{
        path: write_temp(manifest),
        filename: "manifest.yaml",
        content_type: "application/x-yaml"
      }

      conn =
        conn
        |> auth()
        |> post("/api/v1/types/test/greeter/deploy", %{manifest: upload})

      body = json_response(conn, 422)
      assert body["error"] =~ "namespace"
    end

    test "returns 422 when type mismatches URL", %{conn: conn} do
      manifest = """
      version: "1"
      namespace: test
      type: wrong
      """

      upload = %Plug.Upload{
        path: write_temp(manifest),
        filename: "manifest.yaml",
        content_type: "application/x-yaml"
      }

      conn =
        conn
        |> auth()
        |> post("/api/v1/types/test/greeter/deploy", %{manifest: upload})

      body = json_response(conn, 422)
      assert body["error"] =~ "type"
    end

    test "returns 400 when manifest part is missing", %{conn: conn} do
      dummy = %Plug.Upload{
        path: write_temp("dummy"),
        filename: "other.txt",
        content_type: "text/plain"
      }

      conn =
        conn
        |> auth()
        |> post("/api/v1/types/test/greeter/deploy", %{other: dummy})

      assert json_response(conn, 400)["error"] == "missing manifest part"
    end

    test "requires admin scope", %{conn: conn} do
      upload = %Plug.Upload{
        path: write_temp(@valid_manifest),
        filename: "manifest.yaml",
        content_type: "application/x-yaml"
      }

      conn =
        conn
        |> auth(@rw_key)
        |> post("/api/v1/types/test/greeter/deploy", %{manifest: upload})

      assert conn.status == 403
    end
  end

  defp write_temp(content) do
    path = Path.join(System.tmp_dir!(), "quanta_test_#{Quanta.ULID.generate()}")
    File.write!(path, content)
    path
  end
end
