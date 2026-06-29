/**
 * Handler to be executed while sending an email notification.
 * @param {Event} event - Details about the user and the context in which they are logging in.
 * @param {CustomEmailProviderAPI} api - Methods and utilities to help change the behavior of sending a email notification.
 */
exports.onExecuteCustomEmailProvider = async (event, api) => {
  const payload = {
    from: event.notification.from,
    to: event.notification.to || event.user.email,
    subject: event.notification.subject,
    html: event.notification.html,
    text: event.notification.text,
    messageType: event.notification.message_type,
    userId: event.user.user_id,
  };

  try {
    const response = await fetch(event.secrets.CLOUDFLARE_EMAIL_WORKER_URL, {
      method: "POST",
      headers: {
        Authorization: `Bearer ${event.secrets.CLOUDFLARE_EMAIL_WORKER_TOKEN}`,
        "Content-Type": "application/json",
      },
      body: JSON.stringify(payload),
    });

    if (response.ok) {
      return;
    }

    const body = await response.text();
    const message = `Cloudflare email worker failed with ${response.status}: ${body}`;

    if (response.status === 429 || response.status >= 500) {
      api.notification.retry(message);
      return;
    }

    api.notification.drop(message);
  } catch (error) {
    api.notification.retry(`Cloudflare email worker request failed: ${error.message}`);
  }
};

