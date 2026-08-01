export type AdminSession = {
  username: string;
  email: string;
  instance_name: string;
};

export type AdminReportTarget = {
  id: string;
  username: string;
  display_name: string;
  acct: string;
};

export type AdminReport = {
  id: string;
  category: string;
  comment: string;
  created_at: string;
  forwarded: boolean;
  action_taken: boolean;
  action_taken_at: string | null;
  status_ids: string[];
  target_account: AdminReportTarget;
};

export type AdminEmoji = {
  id: string;
  shortcode: string;
  url: string;
  static_url: string;
  visible_in_picker: boolean;
  category: string | null;
};

export type AdminDelivery = {
  id: string;
  source: "outbound" | "outbox";
  account_id: string;
  activity_type: string;
  state: string;
  attempt_count: number;
  target_inbox: string | null;
  last_attempt_at: string | null;
  next_attempt_at: string | null;
  created_at: string;
  updated_at: string;
};

type ApiFetchInit = RequestInit & { parseJson?: boolean };

async function apiFetch<T>(path: string, init: ApiFetchInit = {}): Promise<T> {
  const { parseJson = true, ...requestInit } = init;
  const response = await fetch(path, {
    credentials: "same-origin",
    headers: {
      Accept: "application/json",
      ...(requestInit.headers ?? {}),
    },
    ...requestInit,
  });
  if (!response.ok) {
    const message = await response.text();
    throw new Error(message || `request failed with ${response.status}`);
  }
  if (!parseJson || response.status === 204) {
    return undefined as T;
  }
  return (await response.json()) as T;
}

export function fetchSession(): Promise<AdminSession> {
  return apiFetch<AdminSession>("/api/cfwdon/admin/me");
}

export function fetchReports(status: "all" | "pending" = "all"): Promise<AdminReport[]> {
  const query = status === "pending" ? "?status=pending" : "";
  return apiFetch<AdminReport[]>(`/api/cfwdon/admin/reports${query}`);
}

export function resolveReport(reportId: string): Promise<AdminReport> {
  return apiFetch<AdminReport>(`/api/cfwdon/admin/reports/${reportId}/resolve`, {
    method: "POST",
  });
}

export function fetchEmojis(): Promise<AdminEmoji[]> {
  return apiFetch<AdminEmoji[]>("/api/cfwdon/admin/emojis");
}

export function createEmoji(formData: FormData): Promise<AdminEmoji> {
  return apiFetch<AdminEmoji>("/api/cfwdon/admin/emojis", {
    method: "POST",
    body: formData,
  });
}

export function updateEmoji(
  emojiId: string,
  payload: { visible_in_picker?: boolean; category?: string | null },
): Promise<AdminEmoji> {
  return apiFetch<AdminEmoji>(`/api/cfwdon/admin/emojis/${emojiId}`, {
    method: "PATCH",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(payload),
  });
}

export function deleteEmoji(emojiId: string): Promise<void> {
  return apiFetch<void>(`/api/cfwdon/admin/emojis/${emojiId}`, {
    method: "DELETE",
    parseJson: false,
  });
}

export function fetchDeliveries(state?: string): Promise<AdminDelivery[]> {
  const query = state ? `?state=${encodeURIComponent(state)}` : "";
  return apiFetch<AdminDelivery[]>(`/api/cfwdon/admin/deliveries${query}`);
}

export function retryDelivery(
  deliveryId: string,
  source: AdminDelivery["source"],
): Promise<void> {
  return apiFetch<void>(
    `/api/cfwdon/admin/deliveries/${deliveryId}/retry?source=${source}`,
    { method: "POST", parseJson: false },
  );
}
