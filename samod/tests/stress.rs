use std::time::Duration;

use automerge::transaction::Transactable;
use automerge::{Automerge, ROOT};
use futures::future::join_all;
use samod::storage::TokioFilesystemStorage;
use samod::{
    AcceptorHandle, AlwaysAnnounce, AnnouncePolicy, BackoffConfig, ConcurrencyConfig,
    LocalAnnouncePolicy, NeverAnnounce, PeerId, Repo, Transport, Url,
};
use tokio::net::{TcpListener, TcpStream};
use tokio::select;
use tokio::time::Instant;
use tokio_stream::StreamExt;
use tokio_util::sync::CancellationToken;

fn init_logging() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter("info,samod=off,samod_core=off")
        .try_init();
}

fn handle_server_connection(acceptor: AcceptorHandle, socket: TcpStream) {
    tokio::spawn({
        async move {
            let Ok(_) = acceptor.accept(Transport::from_tokio_io(socket)) else {
                tracing::error!("Error: Acceptor couldn't accept!");
                return;
            };
        }
    });
}

async fn create_repo<A>(announce_policy: A) -> Repo
where
    A: AnnouncePolicy,
{
    const NUM_THREADS: usize = 4;
    let data_dir = std::env::temp_dir().join(rand::random::<u128>().to_string());
    let storage = TokioFilesystemStorage::new(data_dir);

    Repo::build_tokio()
        .with_concurrency(ConcurrencyConfig::Threadpool(
            rayon::ThreadPoolBuilder::new()
                .num_threads(NUM_THREADS)
                .build()
                .unwrap(),
        ))
        .with_peer_id(PeerId::from("sisko"))
        .with_storage(storage)
        .with_announce_policy(announce_policy)
        .load()
        .await
}

/// Starts a server on an OS-assigned port, returning the repo and the URL to dial it on.
async fn create_server(token: CancellationToken) -> (Repo, Url) {
    let repo = create_repo(NeverAnnounce).await;
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = Url::parse(&format!("tcp://{}", listener.local_addr().unwrap())).unwrap();
    let acceptor = repo.make_acceptor(url.clone()).unwrap();
    tokio::spawn(async move {
        loop {
            select! {
                res = listener.accept() => {
                    match res {
                        Ok((socket, _)) => handle_server_connection(acceptor.clone(), socket),
                        Err(e) => tracing::error!("couldn't get client: {:?}", e),
                    }
                },
                _  = token.cancelled() => {
                    break;
                }
            }
        }
    });
    (repo, url)
}

fn make_bytes(size: usize) -> Vec<u8> {
    let mut data = Vec::with_capacity(size);

    for i in 0..size {
        data.push((i % 256) as u8);
    }

    data
}

#[tokio::test]
async fn stress_1() {
    init_logging();
    const KiB: usize = 1024;
    const MiB: usize = 1024 * KiB;
    const ITERATIONS: usize = 10;
    const NUM_DOCS: usize = 1000;
    const DOC_SIZE: usize = 10;

    let mut doc: Automerge = Automerge::new();
    let mut tx = doc.transaction();
    tx.put(ROOT, "data", make_bytes(DOC_SIZE)).unwrap();
    let head = tx.commit().0.unwrap();

    let token = CancellationToken::new();
    let mut times = Vec::new();
    // start the server
    let (_server, url) = create_server(token.clone()).await;

    // now do the client connections
    for _ in 0..ITERATIONS {
        let repo = create_repo(AlwaysAnnounce).await;

        tracing::info!("Dialing...");
        let dial_start = Instant::now();
        let dialer = repo
            .dial_tcp(url.clone(), BackoffConfig::default())
            .unwrap();
        let info = dialer.established().await.unwrap();
        let dial_time = dial_start.elapsed();

        tracing::info!("Connection established.");

        let create_start = Instant::now();
        let (_, mut changes) = repo.connected_peers();

        let mut docs = Vec::new();
        for _ in 0..NUM_DOCS {
            let doc = repo.create(doc.clone()).await.unwrap();
            docs.push(doc);
        }
        let create_time = create_start.elapsed();

        tracing::info!("Starting sync.");
        let sync_start = Instant::now();
        // Before we continue, wait for our heads to sync
        join_all(
            docs.iter()
                .map(|handle| handle.they_have_our_changes(dialer.connection_id().unwrap())),
        )
        .await;
        let sync_time = sync_start.elapsed();

        let total_time = dial_start.elapsed();
        tracing::info!("Dial time: {:?}", dial_time);
        tracing::info!("Create time: {:?}", create_time);
        tracing::info!("Sync time: {:?}", sync_time);
        tracing::info!("Total time: {:?}", total_time);
        times.push(vec![dial_time, create_time, sync_time, total_time]);
        dialer.close();
        repo.stop().await;
    }

    tracing::info!("Times: {:#?}", times);
    token.cancel();
}

#[tokio::test(flavor = "multi_thread")]
async fn stress_2() {
    init_logging();
    const KiB: usize = 1024;
    const MiB: usize = 1024 * KiB;
    const NUM_DOCS: usize = 1000;
    const DOC_SIZE: usize = 10;

    let mut doc: Automerge = Automerge::new();
    let mut tx = doc.transaction();
    tx.put(ROOT, "data", make_bytes(DOC_SIZE)).unwrap();
    let head = tx.commit().0.unwrap();

    // Override with e.g. STRESS_CLIENT_COUNTS=1,5,10,20,50,100
    let client_counts: Vec<usize> = std::env::var("STRESS_CLIENT_COUNTS")
        .map(|s| s.split(',').map(|n| n.trim().parse().unwrap()).collect())
        .unwrap_or_else(|_| vec![1, 5, 10, 20]);
    let mut times = Vec::new();

    for clients in client_counts {
        let token = CancellationToken::new();
        // start the server
        let (repo, url) = create_server(token.clone()).await;
        let mut docs = Vec::new();

        for _ in 0..NUM_DOCS {
            let doc = repo.create(doc.clone()).await.unwrap();
            docs.push(doc.document_id().clone());
        }

        // now do the client connections
        let mut tasks: Vec<tokio::task::JoinHandle<(Duration, Duration, Duration)>> = Vec::new();
        for i in 0..clients {
            let docs = docs.clone();
            let url = url.clone();
            tasks.push(tokio::task::spawn(async move {
                let repo = create_repo(AlwaysAnnounce).await;

                tracing::info!("Dialing {i}...");
                let dial_start = Instant::now();
                let dialer = repo.dial_tcp(url, BackoffConfig::default()).unwrap();
                let info = dialer.established().await.unwrap();
                let dial_time = dial_start.elapsed();

                tracing::info!("Connection established for {i}");

                tracing::info!("Starting sync for {i}");
                let sync_start = Instant::now();
                // Before we continue, wait for our heads to sync
                {
                    let repo = repo.clone();
                    join_all(docs.iter().map(|id| async {
                        let res = repo.find(id.clone()).await;
                        let handle = res.unwrap().unwrap();
                        tracing::info!("Found doc {} for {i}!", handle.document_id());
                    }))
                    .await;
                }

                let sync_time = sync_start.elapsed();

                let total_time = dial_start.elapsed();
                tracing::info!("Dial time: {:?}", dial_time);
                tracing::info!("Sync time: {:?}", sync_time);
                tracing::info!("Total time: {:?}", total_time);
                dialer.close();
                repo.stop().await;

                (dial_time, sync_time, total_time)
            }));
        }

        let results: Vec<_> = join_all(tasks)
            .await
            .into_iter()
            .map(|x| x.unwrap())
            .collect();

        let min_total_time = results.iter().map(|x| x.2).min().unwrap();
        let max_total_time = results.iter().map(|x| x.2).max().unwrap();
        let avg_total_time =
            results.iter().map(|x| x.2).sum::<Duration>().as_secs_f64() / (results.len() as f64);

        tracing::info!("clients: {clients}");
        tracing::info!("min_total_time: {min_total_time:?}");
        tracing::info!("max_total_time: {max_total_time:?}");
        tracing::info!("avg_total_time: {avg_total_time}");
        times.push(vec![
            clients as f64,
            min_total_time.as_secs_f64(),
            max_total_time.as_secs_f64(),
            avg_total_time,
        ]);
        token.cancel();
    }

    tracing::info!("times: {times:#?}");
}
