-- Drop remote_actors whose stored domain does not match actor_uri authority.
-- GLOB patterns require a host boundary after the domain (/, :, ?, #, or EOS).

DELETE FROM remote_actors
WHERE trim(coalesce(domain, '')) = ''
   OR trim(coalesce(actor_uri, '')) = ''
   OR NOT (
        lower(actor_uri) GLOB ('*://' || lower(domain) || '/*')
     OR lower(actor_uri) GLOB ('*://' || lower(domain) || ':*')
     OR lower(actor_uri) GLOB ('*://' || lower(domain) || '?*')
     OR lower(actor_uri) GLOB ('*://' || lower(domain) || '#*')
     OR lower(actor_uri) GLOB ('*://' || lower(domain))
   );
