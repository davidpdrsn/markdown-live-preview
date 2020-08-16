use comrak::{markdown_to_html, ComrakOptions, ComrakRenderOptions};
use crossbeam::channel::unbounded;
use crossbeam::channel::Receiver;
use futures::sink::SinkExt;
use notify::{watcher, RecursiveMode, Watcher};
use std::sync::Arc;
use std::thread;
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};
use structopt::StructOpt;
use tokio::fs::read_to_string;
use warp::filters::ws;
use warp::{
    http::HeaderValue,
    hyper::{header::CONTENT_TYPE, Body, Response},
    Filter, Rejection,
};

/// Live preview of a markdown file
#[derive(Debug, StructOpt)]
struct Opt {
    /// The port the web server will bind to
    #[structopt(long = "port", short = "p", default_value = "3457")]
    port: u16,

    /// The path to the markdown file
    #[structopt(name = "FILE")]
    file: PathBuf,
}

#[tokio::main]
async fn main() {
    let Opt { port, file } = Opt::from_args();
    // let port = port.unwrap_or(3457);

    let (sender, receive) = unbounded::<()>();
    watch_for_file_changes(file.clone(), sender);

    let root = warp::filters::path::end()
        .and(warp::get())
        .map({
            let file = file.clone();
            move || file.clone()
        })
        .and_then(|file: PathBuf| async move {
            let file_name = file.file_stem().unwrap().to_str().unwrap().to_string();
            let contents = read_to_string("src/index.html").await.unwrap();
            let contents = contents.replace("{file}", &file_name);
            Ok::<_, Rejection>(warp::reply::html(contents))
        });

    let css = warp::path!("style.css")
        .and(warp::get())
        .and(warp::fs::file("src/style.css"));

    let script = warp::path!("script.js")
        .and(warp::get())
        .and_then(move || async move {
            let contents = read_to_string("src/script.js").await.unwrap();
            let contents = contents.replace("{port}", &port.to_string());
            let mut res = Response::new(Body::from(contents));
            res.headers_mut()
                .insert(CONTENT_TYPE, HeaderValue::from_static("text/javascript"));
            Ok::<_, Rejection>(res)
        });

    let icon = warp::path!("favicon.ico")
        .and(warp::get())
        .and(warp::fs::file("src/favicon.ico"));

    let assets = css.or(script).or(icon);

    let markdown = warp::path!("markdown")
        .and(warp::get())
        .map({
            let file = file.clone();
            move || file.clone()
        })
        .and_then(|file| async {
            let markdown = read_to_string(file).await.unwrap();
            let html = compile_markdown(&markdown);
            Ok::<_, Rejection>(warp::reply::html(html))
        });

    let receive = Arc::new(receive);
    let ws_handler = warp::path!("ws")
        .and(ws::ws())
        .and(warp::any().map(move || Arc::clone(&receive)))
        .and_then(|ws: ws::Ws, receive: Arc<Receiver<_>>| async {
            let reply = ws.on_upgrade(move |mut socket| async move {
                loop {
                    let receive = Arc::clone(&receive);
                    tokio::task::spawn_blocking(move || receive.recv().unwrap())
                        .await
                        .unwrap();

                    let msg = ws::Message::text("reload");
                    if let Err(_) = socket.send(msg).await {
                        break;
                    }
                }
            });

            Ok::<_, Rejection>(reply)
        });

    let routes = root.or(assets).or(markdown).or(ws_handler);

    let f = tokio::task::spawn(warp::serve(routes).run(([127, 0, 0, 1], port)));
    open_site(port);
    f.await.unwrap();
}

fn watch_for_file_changes(file: PathBuf, sender: crossbeam::channel::Sender<()>) {
    thread::spawn(move || {
        let (watch_send, watch_rec) = std::sync::mpsc::channel();

        let mut watcher = watcher(watch_send, Duration::from_millis(10)).unwrap();
        watcher.watch(file, RecursiveMode::Recursive).unwrap();
        let mut last_event = Instant::now();

        loop {
            match watch_rec.recv() {
                Ok(_) => {
                    let delay = last_event.elapsed();

                    if delay >= Duration::from_millis(1_000) {
                        if sender.is_empty() {
                            sender.send(()).unwrap();
                        }
                        last_event = Instant::now();
                    }
                }
                Err(e) => panic!("watch error: {:?}", e),
            }
        }
    });
}

fn open_site(port: u16) {
    std::process::Command::new("open")
        .arg(format!("http://localhost:{}", port))
        .spawn()
        .unwrap();
}

fn compile_markdown(markdown: &str) -> String {
    let mut options = ComrakOptions::default();

    options.render = {
        let mut options = ComrakRenderOptions::default();
        options.hardbreaks = true;
        options.github_pre_lang = true;
        options
    };

    markdown_to_html(markdown, &options)
}
