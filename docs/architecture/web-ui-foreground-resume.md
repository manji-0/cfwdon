# Web UI foreground resume

How the `/app` Web UI behaves when a browser tab has been in the background and then returns to the foreground.

<!-- constrained-by ./cfwdon-architecture.md#summary -->
<!-- constrained-by ../planning/durable-objects-candidates.md#1-timeline-streaming-hubs--strong-fit -->

## Previous behavior

Streaming paused reconnect timers while `document.visibilityState` was `hidden`, and reconnected only if the WebSocket object was already gone when the tab became visible. A frozen or back-forward-cached tab could keep a zombie socket, so live events stopped without a reconnect. REST timelines, notifications, conversations, and unread badges were not refreshed on visibility; they only revalidated on remount via `ViewReadiness`. Missed stream events were therefore lost until the user pressed `r` or navigated away and back.

## Desired contract

<!-- dagayn: implemented-by web-ui/src/domain/cache/foreground-resume.ts::ForegroundResume -->

The SPA must not reload. Composer drafts and in-progress overlays stay mounted. Returning to the tab is not a navigation.

A short background (under 30 seconds) keeps a live WebSocket if it is still open. If the socket dropped while hidden, reconnect immediately with backoff reset. Do not refetch REST lists; the stream should have stayed current.

A long background (30 seconds or more), or a back-forward cache restore (`pageshow` with `persisted`), is a catch-up:

1. Force-reconnect the user stream even if a socket object still exists (frozen tabs often leave a dead `OPEN` socket).
2. Refresh unread notification and unread message badges.
3. Soft-revalidate the visible live/collection view without a full-page spinner.
4. If the viewport is scrolled more than 80px from the top, do not replace the list; the user keeps their reading position and can refresh with `r`. Inbox threads and conversation lists always refresh because order is the point of the view.

Streaming remains lossy while disconnected. REST catch-up fills that gap, as Durable Object hubs are not required to buffer missed events.

## Streaming hub

<!-- derived-from #desired-contract -->
<!-- dagayn: implemented-by web-ui/src/infrastructure/streaming/mastodon-stream.ts::createStreamingHub -->

`createStreamingHub` owns visibility and `pageshow` listeners for as long as stream or catch-up subscribers exist. Close handlers ignore sockets that are no longer current, so a forced reconnect cannot be wiped by the old `close` event. Catch-up notifications are debounced by the remount skip window so `visibilitychange` and `pageshow` do not double-fetch.

## Visible views

<!-- derived-from #desired-contract -->
<!-- dagayn: implemented-by web-ui/src/ui/hooks/useForegroundCatchUp.ts::useForegroundCatchUp -->

Mounted home, notifications, public/tag/bookmark/favourite collections, messages, and the open conversation subscribe through `useForegroundCatchUp`. Unread badge providers always catch up. Profile, thread, list, and search pages stay on their existing remount freshness rules for this pass.

## Summary

<!-- derived-from #previous-behavior -->
<!-- derived-from #desired-contract -->
<!-- derived-from #streaming-hub -->
<!-- derived-from #visible-views -->

Short background: keep or restore the stream, no list jump. Long background or bfcache: force-reconnect, refresh badges, and replace the visible list only when the user is near the top.
