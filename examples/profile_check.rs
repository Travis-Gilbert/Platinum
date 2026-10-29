//! Run with tests/profiles/check_native.py. Never touches an installed browser.
extern crate gpui_kit as gpui;
#[cfg(feature = "performance-check")]
#[allow(dead_code)]
#[path = "../src/performance.rs"]
mod performance;

#[allow(dead_code)]
#[path = "../src/engine.rs"]
mod engine;
#[allow(dead_code)]
#[path = "../src/scene.rs"]
mod scene;
use clap::Parser;
use frame_profile::{
    ProfileStore, Session, SessionTab,
    credentials::{self, Keychain},
    migration,
};
use servo::{JSValue, LoadStatus, WebView, WebViewBuilder, WebViewDelegate};
use std::{
    cell::{Cell, RefCell},
    path::PathBuf,
    rc::Rc,
    time::Duration,
};
#[derive(Parser)]
struct Args {
    #[arg(long)]
    root: PathBuf,
    #[arg(long)]
    base: url::Url,
    #[arg(long)]
    reopen: bool,
    #[arg(long)]
    cleanup: bool,
}
#[derive(Default)]
struct Probe {
    painted: Cell<bool>,
    stage: Cell<u8>,
    result: RefCell<Option<bool>>,
}
impl WebViewDelegate for Probe {
    fn notify_load_status_changed(&self, _: WebView, status: LoadStatus) {
        if status == LoadStatus::Started {
            self.painted.set(false);
        }
    }
    fn notify_new_frame_ready(&self, view: WebView) {
        view.paint();
        view.rendering_context().present();
        self.painted.set(true);
    }
    fn notify_crashed(&self, _: WebView, _: String, _: Option<String>) {
        *self.result.borrow_mut() = Some(false);
    }
}
fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let root = args.root.clone();
    let base = args.base.clone();
    let store = ProfileStore::open(&root)?;
    let target = if args.reopen {
        store
    } else {
        store.imported_profile("synthetic-chrome", "Work", "work@example.test")?
    };
    let vault = Keychain::for_profile(&target);
    if args.cleanup {
        let _ = credentials::delete(&target, &vault, base.as_str(), "fixture-user");
        return Ok(());
    }
    if !args.reopen {
        // Exercise CSV import through the same API as the native import picker.
        let csv = root.join("synthetic-passwords.csv");
        std::fs::write(
            &csv,
            format!(
                "name,url,username,password\nFixture,{},fixture-user,fixture-password\n",
                base.join("login")?
            ),
        )?;
        migration::password_csv(&target, &csv, &vault)?;
        std::fs::remove_file(csv)?;
        target.select(target.profile.id)?;
        target.visit(base.join("login")?.as_str(), "Fixture login")?;
    }
    let url = if args.reopen {
        let session = target.session()?;
        anyhow::ensure!(session.tabs.len() == 1, "session was not preserved");
        url::Url::parse(
            session.tabs[0]
                .url
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("no saved tab"))?,
        )?
    } else {
        base.join("login")?
    };
    let script = if args.reopen {
        None
    } else {
        let password = credentials::password(&vault, url.as_str(), "fixture-user")?;
        Some(credentials::fill_script(
            url.as_str(),
            "fixture-user",
            &password,
        )?)
    };
    let profile = target.directory();
    let result = Rc::new(Probe::default());
    let final_result = result.clone();
    let _ = rustls::crypto::ring::default_provider().install_default();
    gpui_kit::application().run(move |cx|{
        engine::Engine::start(&profile,cx);
        let context=Rc::new(scene::SceneRenderingContext::new(dpi::PhysicalSize::new(900,700)).expect("GPU context"));
        let view=WebViewBuilder::new(&engine::Engine::servo(cx),context).url(url)
            .delegate(result.clone()).build();view.show();
        let probe=result.clone();let target=target.clone();let base=base.clone();
        engine::Engine::after_spin(move |cx|{
            if let Some(passed)=*probe.result.borrow() {
                println!("{}",serde_json::json!({"phase":if args.reopen{"reopen"}else{"login"},"passed":passed,"stage":probe.stage.get()}));
                if args.reopen {let _=credentials::delete(&target,&Keychain::for_profile(&target),base.as_str(),"fixture-user");}
                cx.quit();return false;
            }
            if view.load_status()!=LoadStatus::Complete || !probe.painted.get(){return true;}
            match probe.stage.get() {
                0 if args.reopen=>{
                    probe.stage.set(3);let probe=probe.clone();
                    view.evaluate_javascript("!!document.querySelector('#authenticated')",move |r|{*probe.result.borrow_mut()=Some(matches!(r,Ok(JSValue::Boolean(true))));});
                }
                0=>{
                    probe.stage.set(4);let probe=probe.clone();
                    let capture=include_str!("../src/scripts/read_document.js");
                    let script=format!("(() => {{ if (({capture}) !== null) return false; return {}; }})()",script.as_ref().unwrap().as_str());
                    view.evaluate_javascript(script,move |r|{
                        if matches!(r,Ok(JSValue::Boolean(true))){probe.stage.set(1);}else{eprintln!("Fill result: {r:?}");*probe.result.borrow_mut()=Some(false);}
                    });
                }
                1=>{
                    probe.stage.set(2);let probe=probe.clone();
                    view.evaluate_javascript("document.querySelector('form').requestSubmit()",move |r|{if r.is_err(){*probe.result.borrow_mut()=Some(false);}});
                }
                2 if view.url().is_some_and(|u|u.path()=="/signed-in")=>{
                    probe.stage.set(3);let probe=probe.clone();let target=target.clone();let url=view.url().unwrap();
                    view.evaluate_javascript("!!document.querySelector('#authenticated')",move |r|{
                        let passed=matches!(r,Ok(JSValue::Boolean(true)));
                        if passed {target.save_session(&Session{layout:None,tabs:vec![SessionTab{url:Some(url.to_string()),query:None}],active:0}).unwrap();}
                        *probe.result.borrow_mut()=Some(passed);
                    });
                }
                _=>{}
            }
            true
        },cx);
        let probe=result.clone();
        cx.spawn(async move |cx|{
            cx.background_executor().timer(Duration::from_secs(40)).await;
            if probe.result.borrow().is_none(){eprintln!("Native profile acceptance timed out");*probe.result.borrow_mut()=Some(false);cx.update(|cx|cx.quit());}
        }).detach();
    });
    anyhow::ensure!(
        *final_result.result.borrow() == Some(true),
        "native profile acceptance failed"
    );
    Ok(())
}
