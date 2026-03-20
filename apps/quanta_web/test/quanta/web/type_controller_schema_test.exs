defmodule Quanta.Web.TypeControllerSchemaTest do
  use Quanta.Web.ConnCase, async: false

  @schematized_manifest """
  version: "1"
  namespace: test
  type: npc
  state:
    kind: "schematized:npc-state"
  """

  @schematized_manifest_v2 """
  version: "1"
  namespace: test
  type: npc
  state:
    kind: "schematized:npc-state"
    version: 2
  """

  @minimal_wit """
  record npc-state {
      alive: bool,
  }
  """

  @appended_wit """
  record npc-state {
      alive: bool,
      health: u16,
  }
  """

  @changed_type_wit """
  record npc-state {
      alive: u32,
  }
  """

  defp auth(conn, key \\ @admin_key) do
    put_req_header(conn, "authorization", "Bearer #{key}")
  end

  defp deploy_with_wit(conn, manifest_yaml, wit_source) do
    manifest_upload = %Plug.Upload{
      path: write_temp(manifest_yaml),
      filename: "manifest.yaml",
      content_type: "application/x-yaml"
    }

    wit_upload = %Plug.Upload{
      path: write_temp(wit_source),
      filename: "schema.wit",
      content_type: "text/plain"
    }

    conn
    |> auth()
    |> post("/api/v1/types/test/npc/deploy", %{manifest: manifest_upload, wit: wit_upload})
  end

  describe "deploy with schema evolution" do
    test "first deploy with schematized state + WIT succeeds", %{conn: conn} do
      conn = deploy_with_wit(conn, @schematized_manifest, @minimal_wit)
      body = json_response(conn, 200)
      assert body["namespace"] == "test"
      assert body["type"] == "npc"
    end

    test "re-deploy with identical WIT succeeds", %{conn: conn} do
      conn1 = deploy_with_wit(conn, @schematized_manifest, @minimal_wit)
      assert conn1.status == 200

      conn2 = deploy_with_wit(Phoenix.ConnTest.build_conn(), @schematized_manifest, @minimal_wit)
      assert json_response(conn2, 200)["type"] == "npc"
    end

    test "re-deploy with appended field succeeds", %{conn: conn} do
      conn1 = deploy_with_wit(conn, @schematized_manifest, @minimal_wit)
      assert conn1.status == 200

      conn2 = deploy_with_wit(Phoenix.ConnTest.build_conn(), @schematized_manifest, @appended_wit)
      assert json_response(conn2, 200)["type"] == "npc"
    end

    test "re-deploy with changed field type is rejected", %{conn: conn} do
      conn1 = deploy_with_wit(conn, @schematized_manifest, @minimal_wit)
      assert conn1.status == 200

      conn2 = deploy_with_wit(Phoenix.ConnTest.build_conn(), @schematized_manifest, @changed_type_wit)
      body = json_response(conn2, 422)
      assert body["error"] =~ "schema incompatible"
      assert body["error"] =~ "Increment state.version"
    end

    test "re-deploy with changed field type + version bump succeeds", %{conn: conn} do
      conn1 = deploy_with_wit(conn, @schematized_manifest, @minimal_wit)
      assert conn1.status == 200

      conn2 = deploy_with_wit(Phoenix.ConnTest.build_conn(), @schematized_manifest_v2, @changed_type_wit)
      assert json_response(conn2, 200)["type"] == "npc"
    end

    test "deploy without WIT for schematized actor succeeds (no schema check)", %{conn: conn} do
      upload = %Plug.Upload{
        path: write_temp(@schematized_manifest),
        filename: "manifest.yaml",
        content_type: "application/x-yaml"
      }

      conn =
        conn
        |> auth()
        |> post("/api/v1/types/test/npc/deploy", %{manifest: upload})

      assert json_response(conn, 200)["type"] == "npc"
    end
  end

  defp write_temp(content) do
    path = Path.join(System.tmp_dir!(), "quanta_test_#{Quanta.ULID.generate()}")
    File.write!(path, content)
    path
  end
end
