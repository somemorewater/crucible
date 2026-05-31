-- Migration: contract_events table for event indexer (#373)
-- and contract analytics support (#382)

CREATE TABLE IF NOT EXISTS contract_events (
    id                UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    contract_id       TEXT        NOT NULL,
    ledger_sequence   BIGINT      NOT NULL,
    transaction_hash  TEXT        NOT NULL,
    event_type        TEXT        NOT NULL,
    topics            JSONB       NOT NULL DEFAULT '[]',
    data              JSONB       NOT NULL DEFAULT '{}',
    indexed_at        TIMESTAMPTZ NOT NULL DEFAULT NOW(),

    CONSTRAINT uq_contract_events_tx_type UNIQUE (transaction_hash, event_type)
);

CREATE INDEX IF NOT EXISTS idx_contract_events_contract_id
    ON contract_events (contract_id);

CREATE INDEX IF NOT EXISTS idx_contract_events_ledger
    ON contract_events (ledger_sequence DESC);

CREATE INDEX IF NOT EXISTS idx_contract_events_type
    ON contract_events (event_type);
