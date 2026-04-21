use super::*;

#[tokio::test]
async fn broadcast_multiplexes_by_chamber_id() {
    let (tx, mut rx_a) = tokio::sync::broadcast::channel::<SseEvent>(16);
    let mut rx_b = tx.subscribe();
    tx.send(SseEvent::StatusChange {
        chamber_id: "alpha".into(),
    })
    .unwrap();
    let a = rx_a.recv().await.unwrap();
    let b = rx_b.recv().await.unwrap();
    match (a, b) {
        (SseEvent::StatusChange { chamber_id: ca }, SseEvent::StatusChange { chamber_id: cb }) => {
            assert_eq!(ca, "alpha");
            assert_eq!(cb, "alpha");
        }
        _ => panic!("expected StatusChange"),
    }
}
