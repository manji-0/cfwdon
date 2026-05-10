CREATE INDEX IF NOT EXISTS idx_favourites_status_created
    ON favourites (status_id, created_at DESC)
    WHERE status_id IS NOT NULL;

CREATE INDEX IF NOT EXISTS idx_favourites_remote_status_created
    ON favourites (remote_status_id, created_at DESC)
    WHERE remote_status_id IS NOT NULL;

CREATE INDEX IF NOT EXISTS idx_reblogs_status_created
    ON reblogs (status_id, created_at DESC)
    WHERE status_id IS NOT NULL;

CREATE INDEX IF NOT EXISTS idx_reblogs_remote_status_created
    ON reblogs (remote_status_id, created_at DESC)
    WHERE remote_status_id IS NOT NULL;

CREATE INDEX IF NOT EXISTS idx_remote_favourites_status_created
    ON remote_favourites (status_id, created_at DESC);

CREATE INDEX IF NOT EXISTS idx_remote_reblogs_status_created
    ON remote_reblogs (status_id, created_at DESC);

CREATE TRIGGER IF NOT EXISTS trg_favourites_exactly_one_target_insert
BEFORE INSERT ON favourites
WHEN (NEW.status_id IS NULL) = (NEW.remote_status_id IS NULL)
BEGIN
    SELECT RAISE(ABORT, 'favourites require exactly one local or remote status target');
END;

CREATE TRIGGER IF NOT EXISTS trg_favourites_exactly_one_target_update
BEFORE UPDATE ON favourites
WHEN (NEW.status_id IS NULL) = (NEW.remote_status_id IS NULL)
BEGIN
    SELECT RAISE(ABORT, 'favourites require exactly one local or remote status target');
END;

CREATE TRIGGER IF NOT EXISTS trg_bookmarks_exactly_one_target_insert
BEFORE INSERT ON bookmarks
WHEN (NEW.status_id IS NULL) = (NEW.remote_status_id IS NULL)
BEGIN
    SELECT RAISE(ABORT, 'bookmarks require exactly one local or remote status target');
END;

CREATE TRIGGER IF NOT EXISTS trg_bookmarks_exactly_one_target_update
BEFORE UPDATE ON bookmarks
WHEN (NEW.status_id IS NULL) = (NEW.remote_status_id IS NULL)
BEGIN
    SELECT RAISE(ABORT, 'bookmarks require exactly one local or remote status target');
END;

CREATE TRIGGER IF NOT EXISTS trg_reblogs_exactly_one_target_insert
BEFORE INSERT ON reblogs
WHEN (NEW.status_id IS NULL) = (NEW.remote_status_id IS NULL)
BEGIN
    SELECT RAISE(ABORT, 'reblogs require exactly one local or remote status target');
END;

CREATE TRIGGER IF NOT EXISTS trg_reblogs_exactly_one_target_update
BEFORE UPDATE ON reblogs
WHEN (NEW.status_id IS NULL) = (NEW.remote_status_id IS NULL)
BEGIN
    SELECT RAISE(ABORT, 'reblogs require exactly one local or remote status target');
END;

CREATE TRIGGER IF NOT EXISTS trg_statuses_visibility_insert
BEFORE INSERT ON statuses
WHEN NEW.visibility NOT IN ('public', 'unlisted', 'private', 'direct')
BEGIN
    SELECT RAISE(ABORT, 'statuses.visibility must be public, unlisted, private, or direct');
END;

CREATE TRIGGER IF NOT EXISTS trg_statuses_visibility_update
BEFORE UPDATE OF visibility ON statuses
WHEN NEW.visibility NOT IN ('public', 'unlisted', 'private', 'direct')
BEGIN
    SELECT RAISE(ABORT, 'statuses.visibility must be public, unlisted, private, or direct');
END;

CREATE TRIGGER IF NOT EXISTS trg_remote_statuses_visibility_insert
BEFORE INSERT ON remote_statuses
WHEN NEW.visibility NOT IN ('public', 'unlisted', 'private', 'direct')
BEGIN
    SELECT RAISE(ABORT, 'remote_statuses.visibility must be public, unlisted, private, or direct');
END;

CREATE TRIGGER IF NOT EXISTS trg_remote_statuses_visibility_update
BEFORE UPDATE OF visibility ON remote_statuses
WHEN NEW.visibility NOT IN ('public', 'unlisted', 'private', 'direct')
BEGIN
    SELECT RAISE(ABORT, 'remote_statuses.visibility must be public, unlisted, private, or direct');
END;

CREATE TRIGGER IF NOT EXISTS trg_accounts_default_post_visibility_insert
BEFORE INSERT ON accounts
WHEN NEW.default_post_visibility NOT IN ('public', 'unlisted', 'private', 'direct')
BEGIN
    SELECT RAISE(ABORT, 'accounts.default_post_visibility must be public, unlisted, private, or direct');
END;

CREATE TRIGGER IF NOT EXISTS trg_accounts_default_post_visibility_update
BEFORE UPDATE OF default_post_visibility ON accounts
WHEN NEW.default_post_visibility NOT IN ('public', 'unlisted', 'private', 'direct')
BEGIN
    SELECT RAISE(ABORT, 'accounts.default_post_visibility must be public, unlisted, private, or direct');
END;

CREATE TRIGGER IF NOT EXISTS trg_accounts_default_quote_policy_insert
BEFORE INSERT ON accounts
WHEN NEW.default_quote_policy NOT IN ('public', 'followers', 'nobody')
BEGIN
    SELECT RAISE(ABORT, 'accounts.default_quote_policy must be public, followers, or nobody');
END;

CREATE TRIGGER IF NOT EXISTS trg_accounts_default_quote_policy_update
BEFORE UPDATE OF default_quote_policy ON accounts
WHEN NEW.default_quote_policy NOT IN ('public', 'followers', 'nobody')
BEGIN
    SELECT RAISE(ABORT, 'accounts.default_quote_policy must be public, followers, or nobody');
END;

CREATE TRIGGER IF NOT EXISTS trg_statuses_quote_policy_insert
BEFORE INSERT ON statuses
WHEN NEW.quote_approval_policy NOT IN ('public', 'followers', 'nobody')
BEGIN
    SELECT RAISE(ABORT, 'statuses.quote_approval_policy must be public, followers, or nobody');
END;

CREATE TRIGGER IF NOT EXISTS trg_statuses_quote_policy_update
BEFORE UPDATE OF quote_approval_policy ON statuses
WHEN NEW.quote_approval_policy NOT IN ('public', 'followers', 'nobody')
BEGIN
    SELECT RAISE(ABORT, 'statuses.quote_approval_policy must be public, followers, or nobody');
END;

CREATE TRIGGER IF NOT EXISTS trg_statuses_quote_state_insert
BEFORE INSERT ON statuses
WHEN NEW.quote_state NOT IN ('accepted', 'pending', 'rejected', 'revoked')
BEGIN
    SELECT RAISE(ABORT, 'statuses.quote_state must be accepted, pending, rejected, or revoked');
END;

CREATE TRIGGER IF NOT EXISTS trg_statuses_quote_state_update
BEFORE UPDATE OF quote_state ON statuses
WHEN NEW.quote_state NOT IN ('accepted', 'pending', 'rejected', 'revoked')
BEGIN
    SELECT RAISE(ABORT, 'statuses.quote_state must be accepted, pending, rejected, or revoked');
END;

CREATE TRIGGER IF NOT EXISTS trg_remote_statuses_quote_state_insert
BEFORE INSERT ON remote_statuses
WHEN NEW.quote_state NOT IN ('accepted', 'pending', 'rejected', 'revoked')
BEGIN
    SELECT RAISE(ABORT, 'remote_statuses.quote_state must be accepted, pending, rejected, or revoked');
END;

CREATE TRIGGER IF NOT EXISTS trg_remote_statuses_quote_state_update
BEFORE UPDATE OF quote_state ON remote_statuses
WHEN NEW.quote_state NOT IN ('accepted', 'pending', 'rejected', 'revoked')
BEGIN
    SELECT RAISE(ABORT, 'remote_statuses.quote_state must be accepted, pending, rejected, or revoked');
END;

CREATE TRIGGER IF NOT EXISTS trg_media_attachments_focus_insert
BEFORE INSERT ON media_attachments
WHEN (NEW.focus_x IS NOT NULL AND (NEW.focus_x < -1.0 OR NEW.focus_x > 1.0))
  OR (NEW.focus_y IS NOT NULL AND (NEW.focus_y < -1.0 OR NEW.focus_y > 1.0))
BEGIN
    SELECT RAISE(ABORT, 'media_attachments focus values must be between -1.0 and 1.0');
END;

CREATE TRIGGER IF NOT EXISTS trg_media_attachments_focus_update
BEFORE UPDATE OF focus_x, focus_y ON media_attachments
WHEN (NEW.focus_x IS NOT NULL AND (NEW.focus_x < -1.0 OR NEW.focus_x > 1.0))
  OR (NEW.focus_y IS NOT NULL AND (NEW.focus_y < -1.0 OR NEW.focus_y > 1.0))
BEGIN
    SELECT RAISE(ABORT, 'media_attachments focus values must be between -1.0 and 1.0');
END;

CREATE TRIGGER IF NOT EXISTS trg_remote_status_attachments_status_insert
BEFORE INSERT ON remote_status_attachments
WHEN NOT EXISTS (SELECT 1 FROM remote_statuses WHERE id = NEW.status_id)
BEGIN
    SELECT RAISE(ABORT, 'remote_status_attachments.status_id must reference remote_statuses.id');
END;

CREATE TRIGGER IF NOT EXISTS trg_remote_status_attachments_status_update
BEFORE UPDATE OF status_id ON remote_status_attachments
WHEN NOT EXISTS (SELECT 1 FROM remote_statuses WHERE id = NEW.status_id)
BEGIN
    SELECT RAISE(ABORT, 'remote_status_attachments.status_id must reference remote_statuses.id');
END;

CREATE TRIGGER IF NOT EXISTS trg_remote_statuses_delete_attachments
AFTER DELETE ON remote_statuses
BEGIN
    DELETE FROM remote_status_attachments
    WHERE status_id = OLD.id;
END;

CREATE TRIGGER IF NOT EXISTS trg_status_polls_status_insert
BEFORE INSERT ON status_polls
WHEN NOT EXISTS (SELECT 1 FROM statuses WHERE id = NEW.status_id)
BEGIN
    SELECT RAISE(ABORT, 'status_polls.status_id must reference statuses.id');
END;

CREATE TRIGGER IF NOT EXISTS trg_status_polls_status_update
BEFORE UPDATE OF status_id ON status_polls
WHEN NOT EXISTS (SELECT 1 FROM statuses WHERE id = NEW.status_id)
BEGIN
    SELECT RAISE(ABORT, 'status_polls.status_id must reference statuses.id');
END;

CREATE TRIGGER IF NOT EXISTS trg_status_poll_options_poll_insert
BEFORE INSERT ON status_poll_options
WHEN NOT EXISTS (SELECT 1 FROM status_polls WHERE id = NEW.poll_id)
BEGIN
    SELECT RAISE(ABORT, 'status_poll_options.poll_id must reference status_polls.id');
END;

CREATE TRIGGER IF NOT EXISTS trg_status_poll_options_poll_update
BEFORE UPDATE OF poll_id ON status_poll_options
WHEN NOT EXISTS (SELECT 1 FROM status_polls WHERE id = NEW.poll_id)
BEGIN
    SELECT RAISE(ABORT, 'status_poll_options.poll_id must reference status_polls.id');
END;

CREATE TRIGGER IF NOT EXISTS trg_status_poll_votes_poll_insert
BEFORE INSERT ON status_poll_votes
WHEN NOT EXISTS (SELECT 1 FROM status_polls WHERE id = NEW.poll_id)
BEGIN
    SELECT RAISE(ABORT, 'status_poll_votes.poll_id must reference status_polls.id');
END;

CREATE TRIGGER IF NOT EXISTS trg_status_poll_votes_poll_update
BEFORE UPDATE OF poll_id ON status_poll_votes
WHEN NOT EXISTS (SELECT 1 FROM status_polls WHERE id = NEW.poll_id)
BEGIN
    SELECT RAISE(ABORT, 'status_poll_votes.poll_id must reference status_polls.id');
END;

CREATE TRIGGER IF NOT EXISTS trg_status_poll_votes_account_insert
BEFORE INSERT ON status_poll_votes
WHEN NOT EXISTS (SELECT 1 FROM accounts WHERE id = NEW.account_id)
BEGIN
    SELECT RAISE(ABORT, 'status_poll_votes.account_id must reference accounts.id');
END;

CREATE TRIGGER IF NOT EXISTS trg_status_poll_votes_account_update
BEFORE UPDATE OF account_id ON status_poll_votes
WHEN NOT EXISTS (SELECT 1 FROM accounts WHERE id = NEW.account_id)
BEGIN
    SELECT RAISE(ABORT, 'status_poll_votes.account_id must reference accounts.id');
END;

CREATE TRIGGER IF NOT EXISTS trg_status_poll_votes_option_insert
BEFORE INSERT ON status_poll_votes
WHEN NOT EXISTS (
    SELECT 1
    FROM status_poll_options
    WHERE poll_id = NEW.poll_id
      AND position = NEW.option_position
)
BEGIN
    SELECT RAISE(ABORT, 'status_poll_votes.option_position must reference a poll option');
END;

CREATE TRIGGER IF NOT EXISTS trg_status_poll_votes_option_update
BEFORE UPDATE OF poll_id, option_position ON status_poll_votes
WHEN NOT EXISTS (
    SELECT 1
    FROM status_poll_options
    WHERE poll_id = NEW.poll_id
      AND position = NEW.option_position
)
BEGIN
    SELECT RAISE(ABORT, 'status_poll_votes.option_position must reference a poll option');
END;

CREATE TRIGGER IF NOT EXISTS trg_statuses_delete_polls
AFTER DELETE ON statuses
BEGIN
    DELETE FROM status_polls
    WHERE status_id = OLD.id;
END;

CREATE TRIGGER IF NOT EXISTS trg_status_polls_delete_options
AFTER DELETE ON status_polls
BEGIN
    DELETE FROM status_poll_options
    WHERE poll_id = OLD.id;
END;

CREATE TRIGGER IF NOT EXISTS trg_status_polls_delete_votes
AFTER DELETE ON status_polls
BEGIN
    DELETE FROM status_poll_votes
    WHERE poll_id = OLD.id;
END;

CREATE TRIGGER IF NOT EXISTS trg_remote_status_polls_status_insert
BEFORE INSERT ON remote_status_polls
WHEN NOT EXISTS (SELECT 1 FROM remote_statuses WHERE id = NEW.status_id)
BEGIN
    SELECT RAISE(ABORT, 'remote_status_polls.status_id must reference remote_statuses.id');
END;

CREATE TRIGGER IF NOT EXISTS trg_remote_status_polls_status_update
BEFORE UPDATE OF status_id ON remote_status_polls
WHEN NOT EXISTS (SELECT 1 FROM remote_statuses WHERE id = NEW.status_id)
BEGIN
    SELECT RAISE(ABORT, 'remote_status_polls.status_id must reference remote_statuses.id');
END;

CREATE TRIGGER IF NOT EXISTS trg_remote_status_poll_options_poll_insert
BEFORE INSERT ON remote_status_poll_options
WHEN NOT EXISTS (SELECT 1 FROM remote_status_polls WHERE id = NEW.poll_id)
BEGIN
    SELECT RAISE(ABORT, 'remote_status_poll_options.poll_id must reference remote_status_polls.id');
END;

CREATE TRIGGER IF NOT EXISTS trg_remote_status_poll_options_poll_update
BEFORE UPDATE OF poll_id ON remote_status_poll_options
WHEN NOT EXISTS (SELECT 1 FROM remote_status_polls WHERE id = NEW.poll_id)
BEGIN
    SELECT RAISE(ABORT, 'remote_status_poll_options.poll_id must reference remote_status_polls.id');
END;

CREATE TRIGGER IF NOT EXISTS trg_remote_status_poll_votes_poll_insert
BEFORE INSERT ON remote_status_poll_votes
WHEN NOT EXISTS (SELECT 1 FROM remote_status_polls WHERE id = NEW.poll_id)
BEGIN
    SELECT RAISE(ABORT, 'remote_status_poll_votes.poll_id must reference remote_status_polls.id');
END;

CREATE TRIGGER IF NOT EXISTS trg_remote_status_poll_votes_poll_update
BEFORE UPDATE OF poll_id ON remote_status_poll_votes
WHEN NOT EXISTS (SELECT 1 FROM remote_status_polls WHERE id = NEW.poll_id)
BEGIN
    SELECT RAISE(ABORT, 'remote_status_poll_votes.poll_id must reference remote_status_polls.id');
END;

CREATE TRIGGER IF NOT EXISTS trg_remote_status_poll_votes_account_insert
BEFORE INSERT ON remote_status_poll_votes
WHEN NOT EXISTS (SELECT 1 FROM accounts WHERE id = NEW.account_id)
BEGIN
    SELECT RAISE(ABORT, 'remote_status_poll_votes.account_id must reference accounts.id');
END;

CREATE TRIGGER IF NOT EXISTS trg_remote_status_poll_votes_account_update
BEFORE UPDATE OF account_id ON remote_status_poll_votes
WHEN NOT EXISTS (SELECT 1 FROM accounts WHERE id = NEW.account_id)
BEGIN
    SELECT RAISE(ABORT, 'remote_status_poll_votes.account_id must reference accounts.id');
END;

CREATE TRIGGER IF NOT EXISTS trg_remote_status_poll_votes_option_insert
BEFORE INSERT ON remote_status_poll_votes
WHEN NOT EXISTS (
    SELECT 1
    FROM remote_status_poll_options
    WHERE poll_id = NEW.poll_id
      AND position = NEW.option_position
)
BEGIN
    SELECT RAISE(ABORT, 'remote_status_poll_votes.option_position must reference a poll option');
END;

CREATE TRIGGER IF NOT EXISTS trg_remote_status_poll_votes_option_update
BEFORE UPDATE OF poll_id, option_position ON remote_status_poll_votes
WHEN NOT EXISTS (
    SELECT 1
    FROM remote_status_poll_options
    WHERE poll_id = NEW.poll_id
      AND position = NEW.option_position
)
BEGIN
    SELECT RAISE(ABORT, 'remote_status_poll_votes.option_position must reference a poll option');
END;

CREATE TRIGGER IF NOT EXISTS trg_remote_statuses_delete_polls
AFTER DELETE ON remote_statuses
BEGIN
    DELETE FROM remote_status_polls
    WHERE status_id = OLD.id;
END;

CREATE TRIGGER IF NOT EXISTS trg_remote_status_polls_delete_options
AFTER DELETE ON remote_status_polls
BEGIN
    DELETE FROM remote_status_poll_options
    WHERE poll_id = OLD.id;
END;

CREATE TRIGGER IF NOT EXISTS trg_remote_status_polls_delete_votes
AFTER DELETE ON remote_status_polls
BEGIN
    DELETE FROM remote_status_poll_votes
    WHERE poll_id = OLD.id;
END;
