use criterion::{black_box, criterion_group, criterion_main, Criterion};
use ultrafast_models_sdk::{ChatRequest, Message, RoutingStrategy, UltrafastClient};

fn bench_client_creation(c: &mut Criterion) {
    c.bench_function("client_creation_standalone", |b| {
        b.iter(|| {
            let _client = UltrafastClient::standalone()
                .with_openai("test-key")
                .with_anthropic("test-key")
                .with_routing_strategy(RoutingStrategy::LoadBalance {
                    weights: vec![0.5, 0.5],
                })
                .build();
        });
    });

    c.bench_function("client_creation_gateway", |b| {
        b.iter(|| {
            let _client = UltrafastClient::gateway("http://localhost:3000".to_string())
                .with_api_key("test-key")
                .build();
        });
    });
}

fn bench_request_creation(c: &mut Criterion) {
    c.bench_function("chat_request_creation", |b| {
        b.iter(|| {
            let _request = ChatRequest {
                model: "gpt-4".to_string(),
                messages: vec![
                    Message::system("You are a helpful assistant."),
                    Message::user("Hello, how are you?"),
                ],
                temperature: Some(0.7),
                max_tokens: Some(100),
                stream: Some(false),
                ..Default::default()
            };
        });
    });
}

fn bench_message_creation(c: &mut Criterion) {
    c.bench_function("message_creation", |b| {
        b.iter(|| {
            let _user_msg = Message::user("Hello, world!");
            let _assistant_msg = Message::assistant("Hi there!");
            let _system_msg = Message::system("You are a helpful assistant.");
        });
    });
}

fn bench_routing_strategy_creation(c: &mut Criterion) {
    c.bench_function("routing_strategy_creation", |b| {
        b.iter(|| {
            let _single = RoutingStrategy::Single;
            let _fallback = RoutingStrategy::Fallback;
            let _load_balance = RoutingStrategy::LoadBalance {
                weights: vec![0.5, 0.5],
            };
            let _conditional = RoutingStrategy::Conditional { rules: vec![] };
            let _ab_testing = RoutingStrategy::ABTesting { split: 0.5 };
        });
    });
}

fn bench_serialization(c: &mut Criterion) {
    let request = ChatRequest {
        model: "gpt-4".to_string(),
        messages: vec![
            Message::system("You are a helpful assistant."),
            Message::user("Hello, how are you?"),
        ],
        temperature: Some(0.7),
        max_tokens: Some(100),
        stream: Some(false),
        ..Default::default()
    };

    c.bench_function("request_serialization", |b| {
        b.iter(|| {
            let _json = serde_json::to_string(black_box(&request));
        });
    });

    c.bench_function("request_deserialization", |b| {
        let json = serde_json::to_string(&request).unwrap();
        b.iter(|| {
            let _request: ChatRequest = serde_json::from_str(black_box(&json)).unwrap();
        });
    });
}

criterion_group!(
    benches,
    bench_client_creation,
    bench_request_creation,
    bench_message_creation,
    bench_routing_strategy_creation,
    bench_serialization
);
criterion_main!(benches);
