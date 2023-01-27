FROM rust:1.66

COPY . .

RUN cargo build --release

CMD ["./target/release/tweet-worker"]
