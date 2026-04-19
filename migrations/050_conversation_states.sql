CREATE TABLE IF NOT EXISTS conversation_states (
    conversation_id TEXT NOT NULL,
    account_id TEXT NOT NULL,
    last_status_id TEXT,
    unread INTEGER NOT NULL DEFAULT 0,
    deleted_at TEXT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (conversation_id, account_id),
    FOREIGN KEY (conversation_id) REFERENCES conversations(id) ON DELETE CASCADE,
    FOREIGN KEY (account_id) REFERENCES accounts(id) ON DELETE CASCADE,
    FOREIGN KEY (last_status_id) REFERENCES statuses(id) ON DELETE SET NULL
);

CREATE INDEX IF NOT EXISTS idx_conversation_states_account_updated
    ON conversation_states (account_id, updated_at DESC, conversation_id DESC);

INSERT INTO conversation_states (
    conversation_id,
    account_id,
    last_status_id,
    unread,
    deleted_at,
    created_at,
    updated_at
)
SELECT c.id,
       c.owner_account_id,
       c.last_status_id,
       c.unread,
       c.deleted_at,
       c.created_at,
       c.updated_at
FROM conversations c
WHERE NOT EXISTS (
    SELECT 1
    FROM conversation_states cs
    WHERE cs.conversation_id = c.id
      AND cs.account_id = c.owner_account_id
);
