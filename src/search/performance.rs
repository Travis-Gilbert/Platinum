use super::*;

/// Controlled timing harness using the same service as the address bar.
// Source-sharing search examples compile this module but do not run the native harness.
#[allow(dead_code)]
pub fn measure(cx: &App) -> Task<anyhow::Result<serde_json::Value>> {
    let service = cx.global::<SearchService>().clone();
    cx.background_spawn(async move {
        let mut sequential = Vec::new();
        for _ in 0..2 {
            let result = service.search("ocean tides", false)?;
            sequential.push(serde_json::json!({"ms":result.elapsed_ms,"hits":result.hits.len(),"providers":result.providers,"degraded":result.degradations.iter().map(|d|&d.code).collect::<Vec<_>>()}));
        }
        let started = Instant::now();
        let (concurrent, capture_ms) = std::thread::scope(|scope| {
            let tasks: Vec<_> = (0..3).map(|_| {
                let service = &service;
                scope.spawn(move || service.search("ocean tides", false))
            }).collect();
            let capture = scope.spawn(|| {
                std::thread::sleep(std::time::Duration::from_millis(150));
                let started = Instant::now();
                service.capture(VisitedPage {url:Url::parse("https://example.test/tides").unwrap(),html:"<title>Ocean tides</title><p>The moon drives ocean tides.</p>".into()}).unwrap();
                started.elapsed().as_secs_f64()*1000.
            });
            let results: anyhow::Result<Vec<_>> = tasks.into_iter().map(|task| {
                task.join().unwrap().map(|result|serde_json::json!({"ms":result.elapsed_ms,"hits":result.hits.len(),"providers":result.providers,"degraded":result.degradations.iter().map(|d|&d.code).collect::<Vec<_>>()}))
            }).collect();
            (results,capture.join().unwrap())
        });
        Ok(serde_json::json!({"sequential":sequential,"concurrent":concurrent?,"batch_ms":started.elapsed().as_secs_f64()*1000.,"capture_ms":capture_ms}))
    })
}
