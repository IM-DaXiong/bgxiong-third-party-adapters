use bgx_adapter_v2_reference_host::{credential::*, http::*, media::*, policy::*};
use bgx_adapter_v2_reference_host::before_send::{BeforeSendSink, PersistFuture};
use bgx_adapter_v2_ports::wire::HostPortCallV2HttpRequest;
use bgx_adapter_v2_transport::Control;
use bgx_adapter_v2_validator::{hash_bytes, package::*};
use serde_json::{json, Value};
use std::{collections::BTreeMap, sync::{Arc,Mutex}, time::Duration};
use tokio::{io::{AsyncReadExt,AsyncWriteExt},net::TcpListener};
fn broker(origin: &str) -> (tempfile::TempDir, Arc<HttpBroker>) {
    let manifest = json!({"schemaVersion":2,"pluginId":"local.stream","displayName":"Stream test","version":"1.0.0","protocolRange":{"min":2,"max":2},"requiredFeatures":["stream-events-v1"],"optionalFeatures":[],"language":"rust","entry":"adapter.wasm","artifactSha256":hash_bytes(b"test"),"publisherId":null,"publisherKeyId":null,"requestedCapabilities":{"endpointPolicy":["userConfigurable","allowPrivateNetwork"],"networkHosts":[],"allowPrivateNetwork":true,"auth":["none"],"mediaSlots":[],"stateBytes":0,"embeddedHeaders":[]},"limits":{"memoryBytes":33554432,"fuel":1000,"wallMs":100,"outputBytes":1048576}});
    let bytes = serde_json::to_vec(&manifest).unwrap();
    let hashes = BTreeMap::from([
        ("manifest.json".into(), hash_bytes(&bytes)),
        ("adapter.wasm".into(), hash_bytes(b"test")),
    ]);
    let inventory = hashes
        .iter()
        .map(|(k, v)| format!("{v} {k}\n"))
        .collect::<String>();
    let mut trust = TrustContext::default();
    trust.supported_features.insert("stream-events-v1".into());
    let package = verify_package(
        &PackageContents {
            manifest_bytes: &bytes,
            hash_manifest_bytes: inventory.as_bytes(),
            signature: None,
            measured_file_hashes: &hashes,
        },
        &trust,
    )
    .unwrap();
    let scope = LeaseScope {
        package: package.identity().clone(),
        binding_id: "test".into(),
        request_id: "request".into(),
    };
    let policy = NetworkPolicy::new(
        &package,
        scope.clone(),
        DevelopmentNetworkGrant {
            origins: vec![origin.into()],
            allow_private: true,
            allow_embedded_authorization: false,
            denied_hosts: vec![],
            denied_networks: vec![],
        },
        true,
    )
    .unwrap();
    let directory = tempfile::tempdir().unwrap();
    let workspace = DevelopmentWorkspace::create(directory.path()).unwrap();
    let media = MediaAccess {
        broker: Arc::new(Mutex::new(
            MediaBroker::new(workspace, Duration::from_secs(60), None).unwrap(),
        )),
        scope,
    };
    let http = HttpBroker::new(
        policy,
        Arc::new(Mutex::new(CredentialVault::new().unwrap())),
        media,
    )
    .unwrap();
    (directory, Arc::new(http))
}
struct Capture(Arc<Mutex<Vec<Value>>>);
impl BeforeSendSink for Capture {
    fn persist<'a>(&'a self, record: &'a Value) -> PersistFuture<'a> {
        Box::pin(async move { self.0.lock().unwrap().push(record.clone()); Ok(()) })
    }
}
async fn serve() -> (String, tokio::task::JoinHandle<String>) {
    let listener=TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin=format!("http://{}",listener.local_addr().unwrap());
    let task=tokio::spawn(async move {
        let (mut s,_)=listener.accept().await.unwrap();
        let mut bytes=vec![];let mut b=[0];
        while !bytes.ends_with(b"\r\n\r\n") {s.read_exact(&mut b).await.unwrap();bytes.push(b[0]);assert!(bytes.len()<65536);}
        let header=String::from_utf8(bytes).unwrap();
        s.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}").await.unwrap();
        header
    });(origin,task)
}
fn plan(url: &str) -> HostPortCallV2HttpRequest {
    serde_json::from_value(json!({"method":"GET","url":url,"headers":[],"generatedHeaders":[],
      "body":{"kind":"none"},"sink":{"kind":"buffer","maxBytes":1024},"timeoutMs":3000})).unwrap()
}
#[tokio::test]
async fn signed_download_query_must_not_enter_persisted_audit() {
    let (origin,server)=serve().await;
    let (_dir,http)=broker(&origin);
    let records=Arc::new(Mutex::new(vec![]));
    let http=Arc::try_unwrap(http).ok().unwrap().with_before_send(Arc::new(Capture(records.clone())),"test/download".into());
    let secret="synthetic-unregistered-signature-7f82";
    let result=http.execute(&plan(&format!("{origin}/video?X-Amz-Signature={secret}&expires=123")),None,&Control::new(None)).await.unwrap();
    assert_eq!(serde_json::to_value(result).unwrap()["status"],200);
    assert!(server.await.unwrap().contains(secret),"Transport must preserve the signed URL");
    let records=records.lock().unwrap();
    assert_eq!(records.len(),1,"Must exercise actual before-send sink");
    assert!(!records[0].to_string().contains(secret),"SDK persisted an unregistered signed-query value");
    assert!(!serde_json::to_string(&http.audit().unwrap()).unwrap().contains(secret),"HTTP audit leaked signed-query value");
}
#[tokio::test]
async fn ordinary_http_preserves_query_and_records_before_send() {
    let (origin,server)=serve().await;
    let (_dir,http)=broker(&origin);
    let records=Arc::new(Mutex::new(vec![]));
    let http=Arc::try_unwrap(http).ok().unwrap().with_before_send(Arc::new(Capture(records.clone())),"test/query".into());
    let result=http.execute(&plan(&format!("{origin}/tasks?task_ids=fake-task")),None,&Control::new(None)).await.unwrap();
    assert_eq!(serde_json::to_value(result).unwrap()["status"],200);
    assert!(server.await.unwrap().contains("task_ids=fake-task"));
    assert_eq!(records.lock().unwrap().len(),1);
}
