# OpenTelemetry Tracing Reconnaissance Report - Issue #251

**Date:** 2026-04-29  
**Target:** Crucible Backend (`backend/` directory)  
**Objective:** Implement production-grade OpenTelemetry tracing with full instrumentation

---

## 1. Current Backend Structure

### Dependencies (from Cargo.toml)
- **Axum:** 0.7 (HTTP framework)
- **SQLx:** 0.8 (PostgreSQL with tokio-rustls runtime)
- **Redis:** 0.27 (with tokio-comp, json features)
- **Tokio:** 1.x (async runtime with full features)
- **Tracing:** 0.1 (basic tracing)
- **Tracing-Subscriber:** 0.3 (with env-filter)
- **OpenTelemetry:** 0.24 (trace, metrics features)
- **OpenTelemetry-OTLP:** 0.17 (trace, grpc-tonic)
- **OpenTelemetry-Semantic-Conventions:** 0.16
- **OpenTelemetry-SDK:** 0.24 (trace, rt-tokio)
- **Tracing-OpenTelemetry:** 0.25
- **Tonic:** 0.12 (gRPC)

### Service Architecture
```
backend/
├── src/
│   ├── main.rs                    # Server initialization, Axum router
│   ├── config.rs                  # Configuration management
│   ├── error.rs                   # Error types
│   ├── jobs.rs                    # Async job handlers (Apalis)
│   ├── telemetry.rs              # Telemetry utilities
│   ├── api/
│   │   └── handlers/
│   │       ├── profiling.rs      # Metrics & health endpoints
│   │       └── stellar.rs        # Stellar TOML endpoint
│   ├── db/
│   │   ├── mod.rs                # Database utilities
│   │   └── seeds.rs              # Database seeding
│   ├── services/
│   │   ├── tracing.rs            # ✅ ALREADY IMPLEMENTED
│   │   ├── sys_metrics.rs        # System metrics collection
│   │   ├── error_recovery.rs     # Error recovery manager
│   │   ├── log_aggregator.rs     # Log aggregation
│   │   ├── log_alerts.rs         # Log alerting
│   │   └── feature_flags.rs      # Feature flags (Redis + PostgreSQL)
│   └── utils/
│       ├── serialization.rs      # Serialization utilities
│       ├── validation.rs         # Validation utilities
│       └── xdr.rs                # Stellar XDR utilities
```

---

## 2. Services to Instrument (12 identified)

### HTTP Handlers (6 endpoints)
1. **`profiling::get_metrics`** - GET `/api/v1/profiling/metrics`
   - ✅ Already instrumented with `#[instrument]`
   - ✅ Uses `TracingService::service_method_span`
   - Calls: MetricsExporter service

2. **`profiling::get_health`** - GET `/api/v1/profiling/health`
   - ✅ Already instrumented with `#[instrument]`
   - ✅ Uses `TracingService::db_query_span` for DB health check
   - Calls: PostgreSQL health check

3. **`profiling::get_prometheus_metrics`** - GET `/api/v1/profiling/prometheus`
   - ✅ Already instrumented with `#[instrument]`
   - No external calls (static response)

4. **`profiling::get_system_status`** - GET `/api/status`
   - ✅ Already instrumented with `#[instrument]`
   - ✅ Uses `TracingService::service_method_span`
   - Calls: MetricsExporter, ErrorManager

5. **`profiling::trigger_profile_collection`** - POST `/api/profile`
   - ✅ Already instrumented with `#[instrument]`
   - No external calls (generates UUID)

6. **`stellar::get_stellar_toml`** - GET `/.well-known/stellar.toml`
   - ✅ Already instrumented with `#[instrument]`
   - No external calls (static response)

### Service Methods (6 services)
7. **`MetricsExporter::get_metrics`** - System metrics retrieval
   - ⚠️ Needs instrumentation
   - Type: Internal service call

8. **`MetricsExporter::update_metrics`** - Metrics update
   - ⚠️ Needs instrumentation
   - Type: Internal service call

9. **`ErrorManager::get_active_tasks`** - Error recovery tasks
   - ⚠️ Needs instrumentation (service exists but not shown in recon)
   - Type: Internal service call

10. **`FeatureFlagService::is_enabled`** - Feature flag check
    - ⚠️ Needs instrumentation
    - Calls: Redis GET, PostgreSQL SELECT
    - Type: Redis + DB operations

11. **`FeatureFlagService::set`** - Feature flag update
    - ⚠️ Needs instrumentation
    - Calls: PostgreSQL UPSERT, Redis DEL
    - Type: DB + Redis operations

12. **`monitor_transaction`** - Async job handler
    - ⚠️ Needs instrumentation
    - Type: Background job (Apalis)

---

## 3. Database Usage Patterns

### PostgreSQL (SQLx)
- **Connection Pool:** `PgPoolOptions` with max 5 connections
- **Query Patterns:**
  - Health check: `SELECT 1`
  - Feature flags: `SELECT enabled FROM feature_flags WHERE key = $1`
  - Feature flag upsert: `INSERT ... ON CONFLICT ... DO UPDATE`
  - Feature flag delete: `DELETE FROM feature_flags WHERE key = $1`

### Redis
- **Connection:** `ConnectionManager` (multiplexed async)
- **Command Patterns:**
  - Cache get: `GET flag:{key}`
  - Cache set: `SET flag:{key} {value} EX 300`
  - Cache delete: `DEL flag:{key}`
  - Cache scan: `KEYS flag:*`
  - Job queue: Apalis RedisStorage

---

## 4. Existing Tracing Infrastructure

### ✅ Already Implemented (`backend/src/services/tracing.rs`)

**TracingService** provides:
- `init(config)` - Initialize OTLP exporter with Jaeger/Zipkin
- `http_request_span()` - HTTP request spans with semantic conventions
- `db_query_span()` - Database query spans
- `redis_command_span()` - Redis command spans
- `service_method_span()` - Service method spans
- `job_span()` - Async job spans
- `record_error()` - Error recording

**TracingConfig** features:
- OTLP endpoint configuration (default: `http://localhost:4317`)
- Environment-based sampling:
  - Dev: 100% sampling
  - Staging: 10% sampling
  - Production: 1% sampling
- Span limits (128 attributes, events, links per span)
- Resource detection (service name, version, environment)

**Current Instrumentation Status:**
- ✅ HTTP handlers use `#[instrument]` macro
- ✅ Some handlers use `TracingService` span factories
- ⚠️ Service methods lack instrumentation
- ⚠️ Feature flag service lacks Redis/DB tracing
- ⚠️ Background jobs lack instrumentation

---

## 5. Instrumentation Targets

### High Priority (User-Facing)
1. **Feature Flag Service** - Redis + PostgreSQL operations
   - `is_enabled()` - Cache hit/miss, DB query
   - `set()` - DB upsert, cache invalidation
   - `delete()` - DB delete, cache invalidation

2. **Background Jobs** - Apalis job queue
   - `monitor_transaction()` - Transaction monitoring job

### Medium Priority (Internal Services)
3. **MetricsExporter** - System metrics
   - `get_metrics()` - Metrics retrieval
   - `update_metrics()` - Metrics update

4. **ErrorManager** - Error recovery
   - `get_active_tasks()` - Active task retrieval

---

## 6. Semantic Conventions

### HTTP Spans
- `http.method` - HTTP method (GET, POST, etc.)
- `http.route` - Route pattern (e.g., `/api/v1/profiling/metrics`)
- `http.status_code` - Response status code
- `http.flavor` - HTTP version (1.1)
- `http.scheme` - Protocol (https)
- `user.id` - User identifier (if available)
- `otel.kind` - Span kind (server)

### Database Spans (PostgreSQL)
- `db.system` - Database system (postgres)
- `db.statement` - SQL query (truncated to 256 chars)
- `db.operation` - Operation type (SELECT, INSERT, UPDATE, DELETE)
- `db.rows_affected` - Number of rows affected
- `otel.kind` - Span kind (client)

### Redis Spans
- `db.system` - Database system (redis)
- `db.redis.command` - Redis command (GET, SET, DEL, etc.)
- `db.redis.key` - Redis key (if applicable)
- `otel.kind` - Span kind (client)

### Service Spans
- `service.name` - Service name (e.g., "FeatureFlagService")
- `service.method` - Method name (e.g., "is_enabled")
- `otel.kind` - Span kind (internal)

### Job Spans
- `job.name` - Job name (e.g., "monitor_transaction")
- `job.id` - Job identifier
- `otel.kind` - Span kind (internal)

---

## 7. Exporter Configuration

### OTLP/gRPC Exporter
- **Endpoint:** `http://localhost:4317` (configurable via `OTLP_ENDPOINT`)
- **Protocol:** gRPC (Tonic)
- **Timeout:** 10 seconds
- **Batch Processing:** Tokio runtime
- **Compatible With:** Jaeger, Zipkin, Grafana Tempo, Honeycomb, etc.

### Sampling Strategy
- **Dev:** 100% (AlwaysOn)
- **Staging:** 10% (TraceIdRatioBased)
- **Production:** 1% (TraceIdRatioBased with ParentBased)

---

## 8. Performance Targets

### Zero Regression Goals
- **p50 Latency:** < +0.5% overhead
- **p95 Latency:** < +1.0% overhead
- **p99 Latency:** < +2.0% overhead
- **Memory:** < +5MB RSS increase
- **CPU:** < +2% CPU usage increase

### Measurement Strategy
- Benchmark with `criterion` (already in dev-dependencies)
- Load tests with existing `backend/tests/load_tests.rs`
- Compare before/after metrics

---

## 9. Testing Strategy

### Unit Tests
- ✅ Span creation tests (already in `tracing.rs`)
- ⚠️ Need: Service instrumentation tests
- ⚠️ Need: Error propagation tests

### Integration Tests
- ⚠️ Need: End-to-end trace validation
- ⚠️ Need: Span hierarchy verification
- ⚠️ Need: Semantic convention compliance

### Load Tests
- ✅ Existing load tests in `backend/tests/load/`
- ⚠️ Need: Performance regression tests

---

## 10. Documentation Requirements

### README Updates
- ✅ Tracing initialization documented in code
- ⚠️ Need: Jaeger setup instructions
- ⚠️ Need: Trace visualization guide
- ⚠️ Need: Production deployment guide

### Code Documentation
- ✅ TracingService fully documented
- ⚠️ Need: Service instrumentation examples
- ⚠️ Need: Custom span attribute guide

---

## 11. Implementation Checklist

### Phase 1: Service Instrumentation ✅
- [x] TracingService implementation (already done)
- [ ] Instrument FeatureFlagService
- [ ] Instrument MetricsExporter
- [ ] Instrument ErrorManager
- [ ] Instrument background jobs

### Phase 2: Testing 🔄
- [ ] Add integration tests for trace validation
- [ ] Add performance benchmarks
- [ ] Run load tests and measure overhead

### Phase 3: Documentation 📝
- [ ] Update README with tracing guide
- [ ] Add Jaeger docker-compose example
- [ ] Document custom span attributes
- [ ] Add troubleshooting guide

### Phase 4: Validation ✅
- [ ] Verify zero performance regression
- [ ] Validate semantic conventions
- [ ] Test error propagation
- [ ] Verify sampling strategies

---

## 12. Deployment Considerations

### Environment Variables
- `OTLP_ENDPOINT` - OTLP exporter endpoint (default: `http://localhost:4317`)
- `ENV` - Deployment environment (dev, staging, production)
- `RUST_LOG` - Log level filter (default: `info,crucible=debug`)

### Jaeger Deployment
- **Development:** Docker Compose with Jaeger all-in-one
- **Production:** Jaeger Collector + Elasticsearch backend
- **UI:** `http://localhost:16686` (Jaeger Query UI)

### Monitoring
- **Traces:** Jaeger UI for trace visualization
- **Metrics:** Prometheus metrics at `/api/v1/profiling/prometheus`
- **Logs:** Structured logging with tracing-subscriber

---

## Summary

**Current Status:**
- ✅ TracingService fully implemented with OTLP exporter
- ✅ HTTP handlers instrumented with `#[instrument]` macro
- ✅ Some handlers use TracingService span factories
- ⚠️ Service methods need instrumentation
- ⚠️ Feature flag service needs Redis/DB tracing
- ⚠️ Background jobs need instrumentation

**Next Steps:**
1. Instrument FeatureFlagService with Redis and DB spans
2. Instrument MetricsExporter and ErrorManager
3. Instrument background jobs (monitor_transaction)
4. Add integration tests for trace validation
5. Run performance benchmarks
6. Update documentation with Jaeger setup

**Estimated Effort:**
- Service instrumentation: 2-3 hours
- Testing: 2-3 hours
- Documentation: 1-2 hours
- **Total:** 5-8 hours

**Risk Assessment:**
- **Low Risk:** TracingService already implemented and tested
- **Low Risk:** Instrumentation is additive (no breaking changes)
- **Medium Risk:** Performance overhead (mitigated by sampling)
- **Low Risk:** Error propagation (handled by TracingService)
