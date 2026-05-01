# Dashboard API Quick Start Guide

## Setup

### 1. Install Dependencies
All required dependencies are already in `Cargo.toml`:
- `axum` - Web framework
- `sqlx` - PostgreSQL driver
- `redis` - Redis client
- `serde` - Serialization
- `utoipa` - OpenAPI documentation

### 2. Environment Variables
```bash
export DATABASE_URL="postgres://user:password@localhost:5432/crucible"
export REDIS_URL="redis://localhost:6379"
export SERVER_PORT=8080
```

### 3. Run Database Migrations
```bash
cd backend
sqlx migrate run
```

This will create:
- `contracts` table
- `transactions` table
- Necessary indexes

### 4. Start the Server
```bash
cargo run -p backend
```

## API Usage

### Get Dashboard Metrics
```bash
curl http://localhost:8080/api/v1/dashboard/metrics
```

**Response:**
```json
{
  "total_contracts": 100,
  "total_transactions": 5000,
  "avg_processing_time_ms": 125.5,
  "failed_transactions_24h": 3,
  "timestamp": "2024-01-15T10:30:00.000Z"
}
```

**Caching:** Results are cached in Redis for 60 seconds.

### Get Contract Statistics
```bash
curl http://localhost:8080/api/v1/dashboard/contracts/my_contract_id/stats
```

**Response:**
```json
{
  "contract_id": "my_contract_id",
  "invocation_count": 42,
  "last_invoked": "2024-01-15T10:25:00.000Z",
  "avg_gas_cost": 1500.75
}
```

**Caching:** Results are cached in Redis for 30 seconds per contract.

**Error Response (404):**
```json
{
  "error": "Contract my_contract_id not found",
  "code": 404
}
```

## Testing

### Run Unit Tests
```bash
cargo test -p backend --lib dashboard
```

### Run Integration Tests
```bash
# Requires PostgreSQL and Redis running
cargo test -p backend --test dashboard_tests
```

### Run Benchmarks
```bash
cargo bench -p backend --bench dashboard_bench
```

## OpenAPI Documentation

Visit the interactive Swagger UI:
```
http://localhost:8080/swagger-ui
```

The dashboard endpoints are documented under the "dashboard" tag.

## Inserting Test Data

```sql
-- Insert test contracts
INSERT INTO contracts (contract_id, name, description) 
VALUES 
  ('contract_1', 'Token Contract', 'ERC20-like token'),
  ('contract_2', 'NFT Contract', 'NFT marketplace');

-- Insert test transactions
INSERT INTO transactions (transaction_hash, contract_id, status, processing_time_ms, gas_cost)
VALUES
  ('tx_001', 'contract_1', 'success', 100.5, 1500.0),
  ('tx_002', 'contract_1', 'success', 120.3, 1600.0),
  ('tx_003', 'contract_2', 'failed', 200.0, 1700.0),
  ('tx_004', 'contract_1', 'success', 95.2, 1450.0);
```

## Monitoring

### Check Redis Cache
```bash
redis-cli
> GET dashboard:metrics
> GET dashboard:contract:contract_1:stats
> TTL dashboard:metrics
```

### Check Database Performance
```sql
-- Check query performance
EXPLAIN ANALYZE SELECT COUNT(*) FROM contracts;
EXPLAIN ANALYZE SELECT COUNT(*) FROM transactions WHERE status = 'failed';

-- Check index usage
SELECT schemaname, tablename, indexname, idx_scan 
FROM pg_stat_user_indexes 
WHERE tablename IN ('contracts', 'transactions');
```

### View Logs
The application uses `tracing` for structured logging:
```bash
RUST_LOG=debug cargo run -p backend
```

Look for log entries like:
```
INFO backend::api::handlers::dashboard: Fetching dashboard metrics
INFO backend::api::handlers::dashboard: Returning cached dashboard metrics
INFO backend::api::handlers::dashboard: Dashboard metrics retrieved contracts=100 transactions=5000
```

## Performance Characteristics

| Operation | Cached | Uncached | Notes |
|-----------|--------|----------|-------|
| Get Metrics | ~2-5ms | ~30-50ms | Depends on DB size |
| Get Contract Stats | ~2-5ms | ~20-40ms | Single contract query |
| Serialization | ~0.5ms | ~0.5ms | Measured by benchmarks |

## Troubleshooting

### "Contract not found" error
- Verify the contract exists: `SELECT * FROM contracts WHERE contract_id = 'your_id';`
- Check for typos in the contract ID
- Ensure transactions exist for that contract

### Slow queries
- Check if indexes are being used: `EXPLAIN ANALYZE <query>`
- Verify PostgreSQL is properly configured
- Consider increasing cache TTL for less frequently changing data

### Cache not working
- Verify Redis is running: `redis-cli PING`
- Check Redis connection in logs
- Verify `REDIS_URL` environment variable

### Database connection errors
- Verify PostgreSQL is running
- Check `DATABASE_URL` format
- Ensure database exists and migrations have run

## Architecture Notes

### State Management
- `DashboardState` is separate from `AppState` (profiling)
- Each has its own database pool and Redis connection
- Allows independent scaling and configuration

### Caching Strategy
- Cache-first approach for read-heavy workloads
- Different TTLs based on data volatility
- Graceful fallback to database on cache miss
- JSON serialization for complex types

### Error Handling
- Uses existing `AppError` enum
- Proper HTTP status codes (200, 404, 500)
- Structured error logging
- User-friendly error messages

## Next Steps

1. **Add more endpoints:**
   - Transaction history for a contract
   - Time-series metrics
   - Top contracts by invocation count

2. **Enhance caching:**
   - Cache invalidation on writes
   - Distributed cache with Redis Cluster
   - Cache warming strategies

3. **Add monitoring:**
   - Prometheus metrics export
   - Grafana dashboards
   - Alert rules for anomalies

4. **Optimize queries:**
   - Add materialized views for complex aggregations
   - Implement query result pagination
   - Add database read replicas
