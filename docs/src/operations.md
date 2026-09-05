# Operating and releasing Cryochamber

## Backup and restore

Back up a stopped system so mailbox moves and config updates cannot race the
copy. Schedule a maintenance window, stop incoming chat bridges and stop the
hub with `cryohub stop`. In each chamber run `cryo cancel`; if native Zulip sync
is enabled, run `cryo-zulip unsync`. For the Python bridge, run `chat-bridge unsync --chamber <path>`. Check that no chamber or bridge process remains.

Copy the entire chamber directory, including `plan.md`, `NOTES.md`, `cryo.toml`,
`todo.json`, `timer.json`, logs, messages, attachments and `.cryo`. Include the
reply-obligation journal if present. Exclude `.cryo/cryo.sock`, which is a runtime
socket. Preserve permissions and symlinks. For example, from the chamber's parent:

```bash
umask 077
tar --exclude='./my-chamber/.cryo/cryo.sock' -czf chamber-backup.tar.gz ./my-chamber
```

Also copy host configuration from `${XDG_CONFIG_HOME:-$HOME/.config}/cryo` and
the chamber registry from `$XDG_STATE_HOME/cryo/chambers` if XDG_STATE_HOME is
set, otherwise `~/.cryo/chambers`. Backups contain API keys and bearer tokens;
store them with restricted access and encryption provided by your backup system.
Native device keys are not portable backups. Issue new hub tokens on a new device.

Restore into a separate directory first, with all services still stopped.
Check the restored config, plan, TODOs and mailbox history. Remove a restored
socket and clear only the process identity in `timer.json`:

```bash
python3 - <<'PY'
import json
from pathlib import Path
path = Path('timer.json')
state = json.loads(path.read_text())
state['pid'] = None
state['instance_id'] = None
path.write_text(json.dumps(state, indent=2))
Path('.cryo/cryo.sock').unlink(missing_ok=True)
PY
```

Preserve session activity, TODO claim state and the reply journal. They tell the
daemon what was interrupted. Start the restored chamber only after disabling the
original instance and checking its runner credentials. Confirm that history is
present, interrupted claims produce a notice, and a newly sent instruction gets
a reply. Then restart the hub and bridges. Never run two copies of the same
restored chamber against the same external service.

Before an upgrade, record the installed version and keep its binary plus a
consistent backup. Roll back the binary and its matching data snapshot together;
do not point an older binary at an unreviewed newer data format.

## History and performance

The Console requests the latest 100 messages and loads earlier pages on demand.
The page cursor uses immutable mailbox filenames, which Cryochamber prefixes with
the send timestamp. Moving a file to archive preserves its cursor. Manually named
legacy files retain lexical page order; timestamps still order messages within
the displayed window. Legacy API clients can still request the full array by
omitting `limit`. A page accepts `limit=1..100` and an optional `before` cursor.

Paging lists filenames but only opens message bodies in the selected window.
Session logs are still read in full. The initial qualification envelope is
20 chambers and 10,000 messages per chamber; this is a test target, not a latency
guarantee. Measure optimized builds on named hardware before publishing an SLO.

## Release checks

Version tags run the normal Rust, Console, Python and native-shell checks plus
dependency auditing. The workflow creates a draft release, builds native assets,
publishes the crate only after successful builds, then publishes the draft.
An unsuccessful run must leave the release draft for investigation. Crate
publication and GitHub release publication are separate operations, so verify
both before retrying a partially completed release.

Before tagging, run the packaged-app smoke checklist in `app/README.md` on
macOS and Android. Record device/OS versions, commit, installer checksums and
results. Include first install, upgrade, cold restart, credential migration,
offline recovery, certificate changes, native Back, keyboard focus and token
revocation. A CI build is not a substitute for these checks.

Customer macOS releases require these GitHub secrets:

- `APPLE_CERTIFICATE`, the base64 Developer ID Application certificate in p12 format.
- `APPLE_CERTIFICATE_PASSWORD` and `APPLE_SIGNING_IDENTITY`.
- `APPLE_API_ISSUER`, `APPLE_API_KEY`, and `APPLE_API_KEY_CONTENT`, the App Store Connect private key contents.

The pipeline verifies the signature, notarization ticket and Gatekeeper
assessment. Local ad-hoc builds remain available for development. Follow the
[Tauri signing guide](https://v2.tauri.app/distribute/sign/macos/) when provisioning
credentials. Android uses the existing release keystore secrets.

Require PRs and passing component/security checks on `main`, prohibit force
pushes and deletion, and require the branch to be current before merge. A solo
maintainer can use zero required human approvals; passing checks remain required.

## Dependency findings

Run the pinned `cargo-audit` scanner against both Cargo lockfiles and `npm audit`
against the Console lockfile. Network failure means the check is incomplete.
Scheduled checks catch new advisories even when dependencies have not changed.

The 2026-09-05 native audit identified unmaintained GTK3 bindings and Unicode
dependencies, and `RUSTSEC-2024-0429` in the Linux-only GLib 0.18 dependency.
macOS and Android are the supported native release targets; a Linux native
release requires resolving that GLib finding first. Track these upstream
dependencies rather than changing incompatible transitive major versions by hand.
Owner: repository maintainer. Reassess by 2026-12-01, or when Tauri changes its
Linux backend. The audit gate denies soundness and yanked-package findings. It exempts only
RUSTSEC-2024-0429 until that date; CI fails when the exception expires. Maintenance
warnings remain visible in audit output. The yanked chacha20 0.10.1 dependency
was updated to 0.10.2 with unchanged dependency requirements.
