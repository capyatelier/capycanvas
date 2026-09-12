use super::*;
use std::path::PathBuf;

struct Library(*mut CapyWorkspaceLibrary);
impl Library {
    fn new(platform: u32, directory: &std::path::Path, scene: &str) -> Self {
        let directory = CString::new(directory.to_str().unwrap()).unwrap();
        let scene = CString::new(scene).unwrap();
        let pointer =
            unsafe { capy_workspace_library_create(platform, directory.as_ptr(), scene.as_ptr()) };
        assert!(!pointer.is_null());
        Self(pointer)
    }
    fn raw(&self, value: Value) -> Value {
        let value = CString::new(value.to_string()).unwrap();
        let reply = unsafe { capy_workspace_library_request(self.0, value.as_ptr()) };
        assert!(!reply.is_null());
        let result = serde_json::from_slice(unsafe { CStr::from_ptr(reply) }.to_bytes()).unwrap();
        unsafe { capy_apple_string_free(reply) };
        result
    }
    fn request(&self, value: Value) -> Value {
        let result = self.raw(value);
        assert!(result.get("error").is_none(), "{result}");
        result
    }
    fn adopt(&self, app: &App, reply: &Value) -> String {
        let value = &reply["value"]["adoption"];
        assert!(value.is_object());
        app.request(6, json!({"type":"adopt","capture":value["capture"]}))
            .unwrap();
        let active = self.request(json!({"type":"activate","token":value["token"]}));
        app.request(
            6,
            json!({"type":"configure","binding":active["value"]["binding"]}),
        )
        .unwrap();
        app.request(6, json!({"type":"end"})).unwrap();
        active["status"]["active_id"].as_str().unwrap().into()
    }
    fn observe(&self, app: &App, now: u64) {
        let value = app
            .request(6, json!({"type":"capture","generation":null}))
            .unwrap();
        self.request(json!({"type":"observe","capture":value["capture"],"working":value["working"],"now":now}));
        self.request(json!({"type":"flush"}));
    }
}
impl Drop for Library {
    fn drop(&mut self) {
        unsafe { capy_workspace_library_destroy(self.0) };
    }
}
struct Directory(PathBuf);
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn apple_workspace_library_handoff_round_trips_history_tools_and_scene_identity() {
    for platform in [0, 1] {
        let directory = Directory(std::env::temp_dir().join(format!(
            "capy-apple-workspaces-{}",
            layer_workspace::new_id()
        )));
        let library = Library::new(platform, &directory.0, "scene:first");
        let app = App::new(platform);
        app.request(6, json!({"type":"begin"})).unwrap();
        let initial = library.request(json!({"type":"initialize","now":1000}));
        let pending = &initial["value"]["adoption"];
        // A wrong acknowledgement and an unrelated operation cannot replace
        // the pending aggregate or start a second storage-side adoption.
        assert!(library.raw(json!({"type":"activate","token":"old"}))["error"].is_object());
        assert!(
            library.raw(
                json!({"type":"operation","operation":{"type":"new","name":"Too early"},"now":1100})
            )["error"]
                .is_object()
        );
        let original = library.adopt(&app, &initial);
        assert_eq!(pending["id"], original);
        let document = app.state()["layers"].clone();
        app.action(json!({"type":"set_brush_size","value":53.}));
        app.action(json!({"type":"customize","action":{"type":"set_panel_visible","panel":"navigator","visible":false}}));
        library.observe(&app, 2000);
        let first_capture = app
            .request(6, json!({"type":"capture","generation":null}))
            .unwrap();
        let retained = app
            .request(
                6,
                json!({"type":"capture","generation":first_capture["generation"]}),
            )
            .unwrap();
        assert!(
            retained["capture"].is_null(),
            "An unchanged layout must not recopy its history"
        );
        assert_eq!(retained["working"], first_capture["working"]);
        app.request(6, json!({"type":"begin"})).unwrap();
        let duplicate = library.request(json!({"type":"operation","operation":{"type":"duplicate","id":original,"name":"Second Workspace"},"now":3000}));
        let second = library.adopt(&app, &duplicate);
        assert_ne!(second, original);
        app.action(json!({"type":"set_brush_size","value":91.}));
        library.observe(&app, 4000);
        app.request(6, json!({"type":"begin"})).unwrap();
        let switched = library.request(
            json!({"type":"operation","operation":{"type":"switch","id":original},"now":5000}),
        );
        library.adopt(&app, &switched);
        assert_eq!(app.state()["brush"]["diameter"], 53.);
        let restored = app
            .request(6, json!({"type":"capture","generation":null}))
            .unwrap();
        // Storage timestamps committed revisions, without altering history IDs,
        // navigation or the selected latest working values.
        assert_eq!(
            restored["capture"]["history"]["current"],
            first_capture["capture"]["history"]["current"]
        );
        assert_eq!(
            restored["capture"]["history"]["undo"],
            first_capture["capture"]["history"]["undo"]
        );
        assert_eq!(app.state()["layers"], document);
        app.invoke("undo_workspace");
        assert!(app.state()["workspace"]["layout"]["panels"].is_array());
        assert_eq!(app.state()["brush"]["diameter"], 53.);
        app.invoke("redo_workspace");
        library.observe(&app, 6000);
        let toolbar = library.request(json!({"type":"operation","operation":{"type":"save_toolbar","panel":"toolbar","name":"Drawing Tools"},"now":7000}));
        let exported = library.request(json!({"type":"export","id":toolbar["value"]["selected"]}));
        let package: Value =
            serde_json::from_str(exported["value"]["text"].as_str().unwrap()).unwrap();
        assert!(package["working"].is_null());
        assert!(package.get("owner").is_none());
        let other = Library::new(platform, &directory.0, "scene:other");
        let other_app = App::new(platform);
        other_app.request(6, json!({"type":"begin"})).unwrap();
        let other_initial = other.request(json!({"type":"initialize","now":7100}));
        let other_id = other.adopt(&other_app, &other_initial);
        assert_ne!(other_id, original);
        let view = json!({"type":"view","page":"workspaces","query":"","idle":true,"now":7200});
        let before_reopen = other.request(view.clone())["value"]["rows"]
            .as_array()
            .unwrap()
            .len();
        library.request(json!({"type":"close"}));
        drop(library);
        let reopened = Library::new(platform, &directory.0, "scene:first");
        app.request(6, json!({"type":"begin"})).unwrap();
        let restored = reopened.request(json!({"type":"initialize","now":8000}));
        assert_eq!(reopened.adopt(&app, &restored), original);
        assert_eq!(app.state()["brush"]["diameter"], 53.);
        assert_eq!(
            other.request(view)["value"]["rows"]
                .as_array()
                .unwrap()
                .len(),
            before_reopen,
            "Restoring a bound scene must not create a spare workspace beside another live window"
        );
        reopened.request(json!({"type":"close"}));
        other.request(json!({"type":"close"}));
    }
}

#[test]
fn apple_workspace_teardown_releases_pending_and_active_claims_without_overwriting_saved_data() {
    for platform in [0, 1] {
        for active in [false, true] {
            let directory = Directory(
                std::env::temp_dir()
                    .join(format!("capy-apple-teardown-{}", layer_workspace::new_id())),
            );
            let library = Library::new(platform, &directory.0, "scene:first");
            let app = App::new(platform);
            app.request(6, json!({"type":"begin"})).unwrap();
            let initial = library.request(json!({"type":"initialize","now":1000}));
            let id = initial["value"]["adoption"]["id"]
                .as_str()
                .unwrap()
                .to_owned();
            if active {
                library.adopt(&app, &initial);
            }
            let saved = library.request(json!({"type":"load","id":id}))["value"]["entity"].clone();
            if active {
                app.action(json!({"type":"set_brush_size","value":83.}));
                let capture = app
                    .request(6, json!({"type":"capture","generation":null}))
                    .unwrap();
                library.request(json!({"type":"observe","capture":capture["capture"],"working":capture["working"],"now":1001}));
            }
            library.request(json!({"type":"detach"}));
            let after = library.request(json!({"type":"load","id":id}));
            assert!(after["value"]["claim"].is_null());
            assert_eq!(after["value"]["entity"], saved);
            let next = Library::new(platform, &directory.0, "scene:next");
            let resumed = next.request(json!({"type":"initialize","now":1002,"preferred":id}));
            assert_eq!(resumed["value"]["adoption"]["id"], id);
            next.request(json!({"type":"detach"}));
        }
    }
}
