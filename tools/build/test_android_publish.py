import copy
import hashlib
import io
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
import urllib.error

import android_publish


VERSION = "1.0.11"
CODE = "1000011"
RELEASE = {
    "name": "Uploaded bundle",
    "status": "completed",
    "versionCodes": [CODE],
    "releaseNotes": [{"language": "en-US", "text": "Painting fixes"},
                     {"language": "fr-FR", "text": "Corrections de peinture"}],
    "inAppUpdatePriority": 3,
}


class PlayFixture:
    def __init__(self, source, target=None, fail_at=None, bundles=None, uploaded=None):
        self.source = {"track": "internal", "releases": copy.deepcopy(source)}
        self.target = {"track": "alpha", "releases": copy.deepcopy(target or [])}
        self.fail_at = fail_at
        self.bundles = bundles or []
        self.uploaded = uploaded
        self.calls = []

    def request(self, method, path, body=None):
        self.calls.append((method, path, copy.deepcopy(body)))
        if (method, path) == self.fail_at:
            raise ValueError("Play rejected the edit")
        if (method, path) == ("POST", "edits"):
            return {"id": "edit"}
        if (method, path) == ("GET", "edits/edit/tracks/internal"):
            return self.source
        if (method, path) == ("GET", "edits/edit/tracks/alpha"):
            return self.target
        if (method, path) == ("GET", "edits/edit/bundles"):
            return {"bundles": self.bundles}
        if (method, path) == ("POST", "edits/edit/bundles?uploadType=media"):
            return self.uploaded
        if (method, path) in (("PUT", "edits/edit/tracks/internal"),
                              ("PUT", "edits/edit/tracks/alpha"),
                              ("POST", "edits/edit:validate"),
                              ("POST", "edits/edit:commit?changesInReviewBehavior=ERROR_IF_IN_REVIEW"),
                              ("DELETE", "edits/edit")):
            return {}
        raise AssertionError(f"Unexpected Play request: {method} {path}")


class AndroidPublishTests(unittest.TestCase):
    def setUp(self):
        self.enterContext(patch("builtins.print"))

    def test_alpha_promotes_only_exact_completed_internal_version(self):
        older = dict(RELEASE, versionCodes=["1000010"])
        play = PlayFixture([older, RELEASE], [older])
        before = copy.deepcopy(play.source)
        android_publish.promote(play, VERSION, "alpha")
        expected = dict(RELEASE, name=VERSION)
        self.assertEqual(play.calls, [
            ("POST", "edits", {}),
            ("GET", "edits/edit/tracks/internal", None),
            ("GET", "edits/edit/tracks/alpha", None),
            ("PUT", "edits/edit/tracks/alpha", {"track": "alpha", "releases": [expected]}),
            ("POST", "edits/edit:validate", None),
            ("POST", "edits/edit:commit?changesInReviewBehavior=ERROR_IF_IN_REVIEW", None),
        ])
        self.assertEqual(play.source, before)

    def test_internal_rollout_preserves_notes_and_priority_without_partial_rollout(self):
        release = dict(RELEASE, status="draft", userFraction=0.25)
        play = PlayFixture([release])
        android_publish.promote(play, VERSION, "internal")
        updated = next(body for method, path, body in play.calls if method == "PUT")
        self.assertEqual(updated, {"track": "internal", "releases": [dict(RELEASE, name=VERSION)]})
        self.assertEqual(play.source["releases"], [release])
        self.assertFalse(any(path.endswith("/alpha") for method, path, body in play.calls))

    def assert_refused(self, play, message):
        with self.assertRaisesRegex(ValueError, message):
            android_publish.promote(play, VERSION, "alpha")
        self.assertEqual(play.calls[-1], ("DELETE", "edits/edit", None))
        self.assertFalse(any(method == "PUT" or ":commit" in path or ":validate" in path
                             for method, path, body in play.calls))

    def test_missing_duplicate_and_mixed_bundle_source_releases_are_refused(self):
        for releases in ([], [dict(RELEASE, versionCodes=["1000010"])],
                         [dict(RELEASE, versionCodes=[CODE, "1000010"])],
                         [dict(RELEASE, versionCodes=[])], [RELEASE, RELEASE]):
            with self.subTest(releases=releases):
                self.assert_refused(PlayFixture(releases), "single-bundle internal release")

    def test_newer_internal_or_alpha_release_is_never_replaced(self):
        newer = dict(RELEASE, versionCodes=["1000012"])
        for source, target in (([RELEASE, newer], []), ([RELEASE], [newer])):
            with self.subTest(source=source, target=target):
                self.assert_refused(PlayFixture(source, target), "newer testing build")

    def test_alpha_requires_completed_internal_release(self):
        for status in ("draft", "inProgress", "halted", None):
            with self.subTest(status=status):
                self.assert_refused(PlayFixture([dict(RELEASE, status=status)]),
                                    "Roll out and test the internal release")

    def test_failed_validation_or_commit_deletes_edit_without_retry_or_fallback(self):
        commit = "edits/edit:commit?changesInReviewBehavior=ERROR_IF_IN_REVIEW"
        for failure in (("POST", "edits/edit:validate"), ("POST", commit)):
            with self.subTest(failure=failure):
                play = PlayFixture([RELEASE], fail_at=failure)
                with self.assertRaisesRegex(ValueError, "Play rejected the edit"):
                    android_publish.promote(play, VERSION, "alpha")
                self.assertEqual(play.calls[-2:], [(*failure, None), ("DELETE", "edits/edit", None)])
                attempts = [path for method, path, body in play.calls if ":commit" in path]
                self.assertEqual(attempts, [commit] if failure[1] == commit else [])
                self.assertEqual([path for method, path, body in play.calls if method == "PUT"],
                                 ["edits/edit/tracks/alpha"])

    def test_source_read_or_track_update_failure_deletes_edit(self):
        for failure in (("GET", "edits/edit/tracks/internal"),
                        ("GET", "edits/edit/tracks/alpha"),
                        ("PUT", "edits/edit/tracks/alpha")):
            with self.subTest(failure=failure):
                play = PlayFixture([RELEASE], fail_at=failure)
                with self.assertRaisesRegex(ValueError, "Play rejected the edit"):
                    android_publish.promote(play, VERSION, "alpha")
                self.assertEqual(play.calls[-1], ("DELETE", "edits/edit", None))
                self.assertFalse(any(":validate" in path or ":commit" in path
                                     for method, path, body in play.calls))

    def test_invalid_version_or_track_never_opens_edit(self):
        for version, track in (("1.0", "alpha"), (VERSION, "production"), (VERSION, "beta")):
            with self.subTest(version=version, track=track):
                play = PlayFixture([RELEASE])
                with self.assertRaises(ValueError):
                    android_publish.promote(play, version, track)
                self.assertEqual(play.calls, [])

    def test_failed_cleanup_preserves_original_play_error(self):
        play = PlayFixture([RELEASE])
        original = ValueError("Changes are already in review")
        request = play.request

        def reject(method, path, body=None):
            if ":commit" in path:
                raise original
            if method == "DELETE":
                raise RuntimeError("Cleanup unavailable")
            return request(method, path, body)

        with patch.object(play, "request", side_effect=reject), \
                self.assertRaises(ValueError) as caught:
            android_publish.promote(play, VERSION, "alpha")
        self.assertIs(caught.exception, original)


class AndroidUploadTests(unittest.TestCase):
    def setUp(self):
        self.enterContext(patch("builtins.print"))
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        self.bundle = Path(directory.name) / "release.aab"
        self.data = b"preserved Android bundle\x00\xff"
        self.bundle.write_bytes(self.data)
        self.metadata = {"versionCode": int(CODE), "sha256": hashlib.sha256(self.data).hexdigest()}

    def play(self, source=None, **kwargs):
        return PlayFixture(source or [], uploaded=kwargs.pop("uploaded", self.metadata), **kwargs)

    def upload(self, play):
        android_publish.upload(play, VERSION, self.bundle, "Release notes\nSecond paragraph")

    def test_upload_drafts_exact_bundle_and_keeps_completed_internal_release(self):
        completed = dict(RELEASE, versionCodes=["1000010"])
        stale = dict(RELEASE, status="draft", versionCodes=["1000009"])
        play = self.play([completed, stale])
        before = copy.deepcopy(play.source)
        self.upload(play)
        expected = {"name": VERSION, "versionCodes": [CODE], "status": "draft",
                    "releaseNotes": [{"language": "en-US", "text": "Release notes\nSecond paragraph"}]}
        self.assertEqual(play.calls, [
            ("POST", "edits", {}),
            ("GET", "edits/edit/tracks/internal", None),
            ("GET", "edits/edit/bundles", None),
            ("POST", "edits/edit/bundles?uploadType=media", self.data),
            ("PUT", "edits/edit/tracks/internal", {"track": "internal", "releases": [completed, expected]}),
            ("POST", "edits/edit:validate", None),
            ("POST", "edits/edit:commit?changesInReviewBehavior=ERROR_IF_IN_REVIEW", None),
        ])
        self.assertEqual(play.source, before)

    def test_recovery_reuses_identical_existing_bundle_without_uploading_again(self):
        play = self.play(bundles=[self.metadata])
        self.upload(play)
        self.assertFalse(any(path.endswith("uploadType=media") for method, path, body in play.calls))
        self.assertTrue(any(method == "PUT" for method, path, body in play.calls))

    def test_existing_completed_identical_version_is_not_replaced_with_draft(self):
        play = self.play([RELEASE], bundles=[self.metadata])
        self.upload(play)
        self.assertFalse(any(method == "PUT" or path.endswith("uploadType=media")
                             for method, path, body in play.calls))
        self.assertEqual(play.source["releases"], [RELEASE])

    def test_wrong_uploaded_version_or_hash_never_updates_track(self):
        for uploaded in (dict(self.metadata, versionCode=1000012),
                         dict(self.metadata, sha256="0" * 64), {"versionCode": int(CODE)}):
            with self.subTest(uploaded=uploaded):
                play = self.play(uploaded=uploaded)
                with self.assertRaisesRegex(ValueError, "version or SHA-256 differs"):
                    self.upload(play)
                self.assertEqual(play.calls[-1], ("DELETE", "edits/edit", None))
                self.assertFalse(any(method == "PUT" or ":commit" in path
                                     for method, path, body in play.calls))

    def test_existing_version_with_different_hash_is_not_overwritten_or_reuploaded(self):
        play = self.play(bundles=[dict(self.metadata, sha256="0" * 64)])
        with self.assertRaisesRegex(ValueError, "version or SHA-256 differs"):
            self.upload(play)
        self.assertEqual(play.calls[-1], ("DELETE", "edits/edit", None))
        self.assertFalse(any(method == "PUT" or path.endswith("uploadType=media")
                             for method, path, body in play.calls))

    def test_newer_internal_version_refuses_before_uploading_bundle(self):
        play = self.play([dict(RELEASE, versionCodes=["1000012"])])
        with self.assertRaisesRegex(ValueError, "newer testing build"):
            self.upload(play)
        self.assertEqual(play.calls, [("POST", "edits", {}),
                                    ("GET", "edits/edit/tracks/internal", None),
                                    ("DELETE", "edits/edit", None)])

    def test_upload_validation_and_commit_failures_discard_edit_without_fallback(self):
        for failure in (("POST", "edits/edit/bundles?uploadType=media"),
                        ("PUT", "edits/edit/tracks/internal"),
                        ("POST", "edits/edit:validate"),
                        ("POST", "edits/edit:commit?changesInReviewBehavior=ERROR_IF_IN_REVIEW")):
            with self.subTest(failure=failure):
                play = self.play(fail_at=failure)
                with self.assertRaisesRegex(ValueError, "Play rejected the edit"):
                    self.upload(play)
                self.assertEqual(play.calls[-1], ("DELETE", "edits/edit", None))
                self.assertLessEqual(sum(path.endswith("uploadType=media")
                                         for method, path, body in play.calls), 1)
                attempts = [path for method, path, body in play.calls if ":commit" in path]
                self.assertEqual(attempts, [failure[1]] if ":commit" in failure[1] else [])

    def test_missing_bundle_and_invalid_version_never_open_edit(self):
        for version, bundle, error in (("1.0", self.bundle, ValueError),
                                       (VERSION, self.bundle.parent / "missing.aab", FileNotFoundError)):
            with self.subTest(version=version, bundle=bundle):
                play = self.play()
                with self.assertRaises(error):
                    android_publish.upload(play, version, bundle, "Notes")
                self.assertEqual(play.calls, [])


class PlayRequestTests(unittest.TestCase):
    def test_media_upload_uses_upload_endpoint_and_preserves_bytes(self):
        payload = b"AAB\x00\xff"
        with patch.object(android_publish.urllib.request, "urlopen", return_value=io.BytesIO(b'{"versionCode": 1000011}')) as urlopen:
            result = android_publish.Play("test-token").request("POST", "edits/edit/bundles?uploadType=media", payload)
        request = urlopen.call_args.args[0]
        self.assertEqual(request.full_url, "https://androidpublisher.googleapis.com/upload/androidpublisher/v3/applications/art.capycanvas.editor/edits/edit/bundles?uploadType=media")
        self.assertEqual(request.get_method(), "POST")
        self.assertEqual(request.data, payload)
        self.assertEqual(request.get_header("Content-type"), "application/octet-stream")
        self.assertEqual(request.get_header("Authorization"), "Bearer test-token")
        self.assertEqual(urlopen.call_args.kwargs, {"timeout": 180})
        self.assertEqual(result, {"versionCode": 1000011})

    def test_json_track_request_and_empty_commit_use_standard_endpoint(self):
        for method, path, body in (("PUT", "edits/edit/tracks/internal", {"track": "internal", "releases": []}),
                                   ("POST", "edits/edit:commit?changesInReviewBehavior=ERROR_IF_IN_REVIEW", None)):
            with self.subTest(path=path), patch.object(android_publish.urllib.request, "urlopen", return_value=io.BytesIO(b"")) as urlopen:
                self.assertEqual(android_publish.Play("test-token").request(method, path, body), {})
            request = urlopen.call_args.args[0]
            self.assertEqual(request.get_method(), method)
            self.assertEqual(request.full_url, f"https://androidpublisher.googleapis.com/androidpublisher/v3/applications/art.capycanvas.editor/{path}")
            self.assertEqual(request.get_header("Content-type"), "application/json")
            self.assertEqual(json.loads(request.data) if body else request.data, body)
            self.assertEqual(urlopen.call_args.kwargs, {"timeout": 60})

    def test_http_error_reports_play_reason_without_retry_or_token(self):
        error = urllib.error.HTTPError("https://example.test", 400, "Bad Request", {},
                                       io.BytesIO(b'{"error":"CHANGES_ALREADY_IN_REVIEW"}'))
        with patch.object(android_publish.urllib.request, "urlopen", side_effect=error) as urlopen:
            with self.assertRaisesRegex(ValueError, "Google Play 400.*CHANGES_ALREADY_IN_REVIEW") as caught:
                android_publish.Play("test-token").request("POST", "edits/edit:commit")
        self.assertNotIn("test-token", str(caught.exception))
        urlopen.assert_called_once()


if __name__ == "__main__":
    unittest.main()
