-- Live streaming subscriptions, so a publish can skip hubs nobody listens on.
-- `hub` is the StreamHub holding the sockets and the only writer of its rows;
-- `target` is the hub the Worker publishes the channel's events to (the same
-- hub, or the open channel hub that forwards to it). `param` is the hashtag or
-- list id, or ''. The primary key leads with `hub` so a hub replaces its own
-- rows with one seek; readers load the whole (small) table.
CREATE TABLE IF NOT EXISTS stream_subscriptions (
    hub TEXT NOT NULL,
    target TEXT NOT NULL,
    stream TEXT NOT NULL,
    param TEXT NOT NULL DEFAULT '',
    PRIMARY KEY (hub, target, stream, param)
) WITHOUT ROWID;
