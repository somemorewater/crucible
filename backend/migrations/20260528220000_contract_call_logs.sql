-- Add contract_call_logs table for contract call logging service
CREATE TABLE IF NOT EXISTS contract_call_logs (
    id SERIAL PRIMARY KEY,
    contract_id VARCHAR(255) NOT NULL,
    function_name VARCHAR(255) NOT NULL,
    arguments JSONB NOT NULL,
    caller VARCHAR(255),
    status VARCHAR(50) NOT NULL CHECK (status IN ('success', 'failed', 'pending')),
    gas_used DOUBLE PRECISION NOT NULL,
    timestamp TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_contract_call_logs_contract_id ON contract_call_logs(contract_id);
CREATE INDEX IF NOT EXISTS idx_contract_call_logs_status ON contract_call_logs(status);
CREATE INDEX IF NOT EXISTS idx_contract_call_logs_timestamp ON contract_call_logs(timestamp);
