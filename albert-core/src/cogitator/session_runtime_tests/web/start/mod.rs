use std::sync::Arc;

use octo_core::{EventBus, Filter, SubscribeOptions};
use octo_history::{AppendRequest, SessionId};
use sha2::{Digest, Sha256};

use super::{actor, configure_web, envelope, next, Fixture};
use crate::{
    cogitator::native_requests::NativeRequest, cogitator::source_context, history::Turn,
    sessions::acceptance::AcceptedStart, sessions::actor::ActorToolPolicy,
};

#[tokio::test]
async fn atomically_accepted_first_input_runs_once_and_publishes_after_reply() {
    let mut fixture = Fixture::with_model("healthy").await;
    configure_web(&mut fixture);
    Arc::get_mut(&mut fixture.host)
        .unwrap()
        .config
        .sessions
        .titles
        .enabled = false;
    let user = actor(&fixture, "first-owner", ActorToolPolicy::ChatOnly).await;
    let seed = fixture
        .service
        .web_create(&user, "Source", false)
        .await
        .unwrap();
    let seed_context = fixture.service.web_context(&user, &seed.id).await.unwrap();
    let request = SessionId::new().to_string();
    let text = "First accepted request";
    let raw = envelope(&seed_context, &request, text);
    let native = NativeRequest::from_envelope(&raw).unwrap();
    let digest = format!("{:x}", Sha256::digest(text.as_bytes()));
    let source = source_context(&raw);
    let row = fixture
        .service
        .web_start_create(
            &user,
            &request,
            "body",
            false,
            AppendRequest {
                namespace: native.namespace,
                idempotency_key: request.clone(),
                digest: digest.clone(),
                turns: vec![Turn::user(format!("{source}\n{text}"))],
            },
        )
        .await
        .unwrap();
    let id = SessionId::parse(&row.session).unwrap();
    let context = fixture.service.web_context(&user, &id).await.unwrap();
    assert!(fixture
        .service
        .web_claim_start(&user, &request)
        .await
        .unwrap());
    let incoming = Arc::new(
        envelope(&context, &request, text).with_context(AcceptedStart::new(
            id.to_string(),
            request.clone(),
            digest,
        )),
    );
    let mut events = fixture
        .ctx
        .bus()
        .subscribe(Filter::by_kind("chat.reply"), SubscribeOptions::default())
        .await
        .unwrap();
    fixture
        .host
        .clone()
        .handle(incoming.clone(), &fixture.ctx)
        .await;
    next(&mut events, "chat.reply").await;
    assert!(!fixture.service.web_unpublished(&id).await.unwrap());
    fixture.host.clone().handle(incoming, &fixture.ctx).await;
    assert_eq!(fixture.server.requests.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn media_first_admission_matches_the_live_prompt_and_runs_exactly_once() {
    use crate::media::{encode, input_digest, MediaInput};
    for temporary in [false, true] {
        let mut fixture = Fixture::with_model("healthy").await;
        configure_web(&mut fixture);
        Arc::get_mut(&mut fixture.host)
            .unwrap()
            .config
            .sessions
            .titles
            .enabled = false;
        let user = actor(&fixture, "media-owner", ActorToolPolicy::ChatOnly).await;
        let seed = fixture
            .service
            .web_create(&user, "Source", false)
            .await
            .unwrap();
        let seed_context = fixture.service.web_context(&user, &seed.id).await.unwrap();
        let request = SessionId::new().to_string();
        let text = "Look at this image";
        let bytes = b"\x89PNG\r\n\x1a\nfixture";
        let asset = fixture
            .service
            .media()
            .stage(&user.account_id, bytes.to_vec(), "image/png", "image.png")
            .await
            .unwrap();
        let raw = envelope(&seed_context, &request, text);
        let native = NativeRequest::from_envelope(&raw).unwrap();
        let digest = input_digest(text, &[asset.id.clone()]);
        let source = source_context(&raw);
        let row = fixture
            .service
            .web_start_create(
                &user,
                &request,
                "media-body",
                temporary,
                AppendRequest {
                    namespace: native.namespace,
                    idempotency_key: request.clone(),
                    digest: digest.clone(),
                    turns: vec![Turn::user(encode(
                        format!("{source}\n{text}"),
                        vec![asset.clone()],
                    ))],
                },
            )
            .await
            .unwrap();
        let id = SessionId::parse(&row.session).unwrap();
        let context = fixture.service.web_context(&user, &id).await.unwrap();
        assert!(fixture
            .service
            .web_claim_start(&user, &request)
            .await
            .unwrap());
        let prepared = fixture
            .service
            .media()
            .prepare(&user.account_id, Some(id.as_str()), &[asset.id.clone()])
            .await
            .unwrap();
        let incoming = Arc::new(
            envelope(&context, &request, text)
                .with_context(AcceptedStart::new(id.to_string(), request, digest))
                .with_context(MediaInput {
                    text: text.into(),
                    assets: prepared.assets(),
                    blobs: prepared.blobs(),
                }),
        );
        let mut events = fixture
            .ctx
            .bus()
            .subscribe(Filter::by_kind("chat.reply"), SubscribeOptions::default())
            .await
            .unwrap();
        fixture
            .host
            .clone()
            .handle(incoming.clone(), &fixture.ctx)
            .await;
        next(&mut events, "chat.reply").await;
        fixture.host.clone().handle(incoming, &fixture.ctx).await;
        assert_eq!(fixture.server.requests.lock().unwrap().len(), 1);
        assert!(!fixture.service.web_unpublished(&id).await.unwrap());
    }
}
