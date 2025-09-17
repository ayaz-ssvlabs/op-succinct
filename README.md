# My op-succinct


To compile the range program:
```bash
cd programs/range/ethereum
cargo prove build
```

To compile the aggregation program:
```bash
cd programs/aggregation
cargo prove build
```

To run my test:
```bash
cd scripts/prove/tests
RUST_LOG=info cargo test -p op-succinct-prove --test range_ethereum_env -- --nocapture
```

Ask me for the `.env` file if needed.
