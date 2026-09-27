# Desktop Calendar

Windows and Ubuntu desktop calendar built with Tauri 2, React, Google Calendar and Google Tasks.

## Run

Prerequisites: Node.js 20+, Rust, and the [Tauri 2 platform prerequisites](https://v2.tauri.app/start/prerequisites/). On Ubuntu, install WebKitGTK 4.1 and AppIndicator development packages and use an X11 session for desktop-layer mode.

Ubuntu build dependencies:

```sh
sudo apt update
sudo apt install libwebkit2gtk-4.1-dev build-essential curl wget file libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev
```

```sh
npm install
npm run tauri dev
```

Build with `npm run tauri build`. Ubuntu builds must be produced on Ubuntu.

## Google setup

1. Create a Google Cloud project. Enable **Google Calendar API** and **Google Tasks API**.
2. Configure the OAuth consent screen. For a personal app in Testing mode, add your Google account as a test user.
3. Create an OAuth client of type **Desktop app**. Save its client ID and client secret when the client is created. If you did not save the secret, add a new secret to the same client in Google Auth Platform > Clients, or create a new Desktop app client.
4. Select **Sign in with Google**. Authentication opens your system browser and returns through a temporary `127.0.0.1` listener.

In PowerShell, create a local `.env` file from the example, replace both placeholder values with credentials from the **same** Desktop app client, and run:

```powershell
if (-not (Test-Path .env)) { Copy-Item .env.example .env }
# Edit .env, then run:
npm.cmd run tauri dev
```

The root `.env` is read by the Rust build and is ignored by Git. Existing shell environment variables take precedence. If you previously got `client_secret is missing`, fully quit the app from the tray and rebuild it after editing `.env`. These values are embedded in the native binary at build time; changing `.env` later requires a rebuild. A desktop binary cannot keep an embedded client secret confidential, so do not treat it as a server-side secret or commit the value to this repository. Keep PKCE enabled.

The app requests `calendar.calendarlist.readonly`, `calendar.events`, and `tasks` scopes. The refresh token is stored in the OS credential store. Offline data is read only. Tasks have date-only due dates in Google's API.

For an external OAuth consent screen left in **Testing** status, Google expires refresh tokens after seven days. Sign in again when prompted, or publish the consent screen for longer-lived personal use. See [Google's OAuth token guidance](https://developers.google.com/identity/protocols/oauth2).

## Behavior

Closing the calendar hides it in the system tray. Click the tray icon or choose **달력 열기 (클릭 가능)** to open an interactive window. Choose **바탕화면에 고정** to keep the widget behind ordinary windows. On Windows this is a clickable bottom-layer window; the WorkerW desktop-icon layer is not used because it intercepted pointer input. The tray also offers sync and quit. The app keeps syncing every five minutes while running. X11 attempts desktop placement, then falls back to a bottom-layer window if it fails. Wayland runs as a normal widget window. New installs start in interactive window mode; enable desktop mode in Settings after signing in.

Recurring events are shown, but editing a recurring instance or series opens Google Calendar. An ordinary event can be created, changed, or deleted in the app. New events can be assigned to a visible, writable calendar; task writes go to the chosen task list.

The sidebar lists available calendars with visibility checkboxes. Unchecking a calendar hides its events from the month grid and selected-day agenda without changing Google Calendar; the choice is saved locally. The task agenda shows only incomplete tasks due on the selected date. Tasks without a due date are not shown in the selected-day agenda.

The month starts on Sunday. Settings offers Midnight, Ocean, Forest, Light, and Vintage themes with color indicators; the choice is restored on launch. The refresh button shows a spinning icon while its request is running. Event and task forms use the native date input; event times use the native time input.

When opening another month, the app displays its SQLite snapshot immediately; snapshots older than five minutes refresh from Google. Each month fetch includes the neighboring dates visible in its grid. Calendar and task-list requests run concurrently. After loading the current month, the app preloads the previous and next months without changing the selected month. New events can be assigned to a visible, writable calendar and the app focuses their saved date.

## Manual verification

- Sign in, quit, relaunch, sync, and sign out. Confirm the account data disappears after sign out.
- Create, edit, and delete an ordinary event and task; complete a task. Compare with the Google web apps.
- Inspect an all-day event crossing midnight, a recurring event, and a task due on the selected day.
- Disconnect the network and reopen a cached month. Confirm edits report the network error.
- Close to tray, reopen by clicking the icon or menu, trigger sync, and quit.
- On Windows and Ubuntu X11, verify widget stacking, opacity, drag/resize persistence, and startup. On Wayland, verify normal-window behavior.
