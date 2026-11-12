-- Accounts and plaintext legacy tokens for scripts/query_count/scenario.mjs.
-- The scenario's first requests migrate the tokens to hashes.
INSERT INTO oauth_apps (id, name, scopes_json, redirect_uris_json, redirect_uri_legacy, client_id, client_secret)
VALUES (1, 'query-count', '["read","write","follow","push"]', '["urn:ietf:wg:oauth:2.0:oob"]',
        'urn:ietf:wg:oauth:2.0:oob', 'query-count-client', 'query-count-secret');
INSERT INTO accounts (id, username, access_email, display_name) VALUES ('100000000000000000', 'qcalice', 'qcalice@example.com', 'QC alice');
INSERT INTO oauth_access_tokens (access_token, oauth_app_id, account_id, scopes_json) VALUES ('qctoken-alice', 1, '100000000000000000', '["read","write","follow","push"]');
INSERT INTO accounts (id, username, access_email, display_name) VALUES ('100000000000000001', 'qcbob', 'qcbob@example.com', 'QC bob');
INSERT INTO oauth_access_tokens (access_token, oauth_app_id, account_id, scopes_json) VALUES ('qctoken-bob', 1, '100000000000000001', '["read","write","follow","push"]');
INSERT INTO accounts (id, username, access_email, display_name) VALUES ('100000000000000002', 'qccarol', 'qccarol@example.com', 'QC carol');
INSERT INTO oauth_access_tokens (access_token, oauth_app_id, account_id, scopes_json) VALUES ('qctoken-carol', 1, '100000000000000002', '["read","write","follow","push"]');
INSERT INTO accounts (id, username, access_email, display_name) VALUES ('100000000000000003', 'qcf1', 'qcf1@example.com', 'QC f1');
INSERT INTO oauth_access_tokens (access_token, oauth_app_id, account_id, scopes_json) VALUES ('qctoken-f1', 1, '100000000000000003', '["read","write","follow","push"]');
INSERT INTO accounts (id, username, access_email, display_name) VALUES ('100000000000000004', 'qcf2', 'qcf2@example.com', 'QC f2');
INSERT INTO oauth_access_tokens (access_token, oauth_app_id, account_id, scopes_json) VALUES ('qctoken-f2', 1, '100000000000000004', '["read","write","follow","push"]');
INSERT INTO accounts (id, username, access_email, display_name) VALUES ('100000000000000005', 'qcf3', 'qcf3@example.com', 'QC f3');
INSERT INTO oauth_access_tokens (access_token, oauth_app_id, account_id, scopes_json) VALUES ('qctoken-f3', 1, '100000000000000005', '["read","write","follow","push"]');
INSERT INTO accounts (id, username, access_email, display_name) VALUES ('100000000000000006', 'qcf4', 'qcf4@example.com', 'QC f4');
INSERT INTO oauth_access_tokens (access_token, oauth_app_id, account_id, scopes_json) VALUES ('qctoken-f4', 1, '100000000000000006', '["read","write","follow","push"]');
INSERT INTO accounts (id, username, access_email, display_name) VALUES ('100000000000000007', 'qcf5', 'qcf5@example.com', 'QC f5');
INSERT INTO oauth_access_tokens (access_token, oauth_app_id, account_id, scopes_json) VALUES ('qctoken-f5', 1, '100000000000000007', '["read","write","follow","push"]');
INSERT INTO accounts (id, username, access_email, display_name) VALUES ('100000000000000008', 'qcf6', 'qcf6@example.com', 'QC f6');
INSERT INTO oauth_access_tokens (access_token, oauth_app_id, account_id, scopes_json) VALUES ('qctoken-f6', 1, '100000000000000008', '["read","write","follow","push"]');
