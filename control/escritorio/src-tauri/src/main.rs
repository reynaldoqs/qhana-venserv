//! Ventana de escritorio del control. Arranca el `qhana-control` incluido en la app, espera a que responda
//! y muestra la interfaz en la ventana. Al cerrar la app se detiene el control.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::{
    fs,
    net::TcpStream,
    process::{Child, Command},
    sync::Mutex,
    time::Duration,
};
use tauri::{Manager, RunEvent};

const DIRECCION: &str = "127.0.0.1:47800";
const URL: &str = "http://127.0.0.1:47800";

struct Control(Mutex<Option<Child>>);

fn responde() -> bool {
    TcpStream::connect_timeout(&DIRECCION.parse().unwrap(), Duration::from_millis(300)).is_ok()
}

fn lanzar(app: &tauri::App) -> Result<Option<Child>, Box<dyn std::error::Error>> {
    // Si ya hay un control en el puerto (otra instancia o uno de desarrollo), se usa ese.
    if responde() {
        return Ok(None);
    }
    let rec = app.path().resource_dir()?.join("recursos");
    let datos = app.path().app_data_dir()?;
    fs::create_dir_all(&datos)?;

    let exe = rec.join(if cfg!(windows) { "qhana-control.exe" } else { "qhana-control" });
    let mut c = Command::new(exe);
    c.current_dir(&datos)
        .env("QHANA_RECETAS", rec.join("recetas"))
        .env("QHANA_UI", rec.join("ui").join("dist"))
        .env("QHANA_APK", rec.join("agente.apk"))
        .env("QHANA_DB", datos.join("control.db"));
    // adb incluido en la app; si no está, el control lo busca en el PATH o en el SDK de Android.
    let adb = rec.join(if cfg!(windows) { "adb.exe" } else { "adb" });
    if adb.exists() {
        c.env("QHANA_ADB", adb);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000); // sin ventana de consola
    }
    Ok(Some(c.spawn()?))
}

fn main() {
    let app = tauri::Builder::default()
        .setup(|app| {
            let hijo = lanzar(app)?;
            app.manage(Control(Mutex::new(hijo)));
            let ventana = app.get_webview_window("main").expect("ventana principal");
            std::thread::spawn(move || {
                for _ in 0..150 {
                    if responde() {
                        let _ = ventana.navigate(URL.parse().unwrap());
                        return;
                    }
                    std::thread::sleep(Duration::from_millis(200));
                }
            });
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("no se pudo crear la app");

    app.run(|handle, evento| {
        if let RunEvent::Exit = evento {
            if let Some(c) = handle.try_state::<Control>() {
                if let Some(mut hijo) = c.0.lock().unwrap().take() {
                    let _ = hijo.kill();
                    let _ = hijo.wait();
                }
            }
        }
    });
}
