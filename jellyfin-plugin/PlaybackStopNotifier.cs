using System;
using System.Linq;
using System.Net.Http;
using System.Net.Http.Headers;
using System.Text;
using System.Text.Json;
using System.Threading.Tasks;
using Jellyfin.Database.Implementations.Entities;
using MediaBrowser.Controller;
using MediaBrowser.Controller.Entities;
using MediaBrowser.Controller.Entities.Audio;
using MediaBrowser.Controller.Events;
using MediaBrowser.Controller.Library;
using MediaBrowser.Model.Entities;
using Microsoft.Extensions.Logging;

namespace Jellyfin.Plugin.BragiScrobbler;

/// <summary>
/// Handles PlaybackStop events and sends scrobbles/skips to Bragi API.
/// </summary>
public class PlaybackStopNotifier : IEventConsumer<PlaybackStopEventArgs>
{
    private readonly ILogger<PlaybackStopNotifier> _logger;
    private readonly IServerApplicationHost _applicationHost;
    private readonly HttpClient _httpClient;

    // Scrobble threshold: 50% of track OR 4 minutes, whichever is less
    private const double MinPlayPercentage = 0.5;
    private const long MaxPlayTimeTicks = 4 * 60 * TimeSpan.TicksPerSecond; // 4 minutes

    /// <summary>
    /// Initialises a new instance of the <see cref="PlaybackStopNotifier"/> class.
    /// </summary>
    public PlaybackStopNotifier(
        ILogger<PlaybackStopNotifier> logger,
        IServerApplicationHost applicationHost,
        IHttpClientFactory httpClientFactory)
    {
        _logger = logger;
        _applicationHost = applicationHost;
        _httpClient = httpClientFactory.CreateClient();
    }

    /// <inheritdoc />
    public async Task OnEvent(PlaybackStopEventArgs eventArgs)
    {
        // Only process audio items
        if (eventArgs.Item is not Audio audioItem)
        {
            return;
        }

        // Skip theme music and trailers
        if (eventArgs.Item.IsThemeMedia)
        {
            return;
        }

        // Must have at least one user
        if (eventArgs.Users.Count == 0)
        {
            return;
        }

        // Get MusicBrainz track ID - required for scrobbling
        var trackMbid = audioItem.ProviderIds.GetValueOrDefault(MetadataProvider.MusicBrainzTrack.ToString());
        if (string.IsNullOrEmpty(trackMbid))
        {
            _logger.LogDebug("Skipping {Name} - no MusicBrainz track ID", audioItem.Name);
            return;
        }

        var config = Plugin.Instance?.Configuration;
        if (config == null)
        {
            _logger.LogWarning("Plugin configuration not available");
            return;
        }

        if (string.IsNullOrEmpty(config.BragiApiUrl) || string.IsNullOrEmpty(config.BragiApiToken))
        {
            _logger.LogDebug("Bragi API not configured");
            return;
        }

        // Calculate play metrics
        var durationTicks = audioItem.RunTimeTicks ?? 0;
        var positionTicks = eventArgs.PlaybackPositionTicks ?? 0;
        var threshold = Math.Min(durationTicks * MinPlayPercentage, MaxPlayTimeTicks);
        var isScrobble = positionTicks >= threshold;

        // Build device ID
        var session = eventArgs.Session;
        var deviceId = $"jellyfin-{(session?.Client ?? "unknown").ToLowerInvariant().Replace(" ", "-")}-{session?.DeviceId ?? "unknown"}";

        // Process for each user in the session
        foreach (var user in eventArgs.Users)
        {
            if (isScrobble && config.EnableScrobbling)
            {
                await SendScrobbleAsync(config, audioItem, trackMbid, positionTicks, deviceId, user);
            }
            else if (!isScrobble && config.EnableSkipTracking && positionTicks > 0)
            {
                await SendSkipAsync(config, audioItem, trackMbid, positionTicks, durationTicks, deviceId, user);
            }
        }
    }

    private async Task SendScrobbleAsync(
        Configuration.PluginConfiguration config,
        Audio audioItem,
        string trackMbid,
        long positionTicks,
        string deviceId,
        User user)
    {
        var artistName = audioItem.Artists?.FirstOrDefault();
        var payload = new
        {
            track_mbid = trackMbid,
            artist_name = artistName,
            track_name = audioItem.Name,
            album_name = audioItem.Album,
            listened_at = DateTime.UtcNow.ToString("o"),
            duration_ms = positionTicks / TimeSpan.TicksPerMillisecond,
            device_id = deviceId,
            jellyfin_item_id = audioItem.Id.ToString("N"),
            user_name = user.Username
        };

        try
        {
            var request = new HttpRequestMessage(HttpMethod.Post, $"{config.BragiApiUrl.TrimEnd('/')}/api/webhooks/bragi/scrobble");
            request.Headers.Authorization = new AuthenticationHeaderValue("Bearer", config.BragiApiToken);
            request.Content = new StringContent(
                JsonSerializer.Serialize(payload),
                Encoding.UTF8,
                "application/json");

            var response = await _httpClient.SendAsync(request);

            if (response.IsSuccessStatusCode)
            {
                _logger.LogInformation("Scrobbled: {Artist} - {Track} (user: {User})",
                    artistName ?? "Unknown",
                    audioItem.Name,
                    user.Username);
            }
            else
            {
                _logger.LogWarning("Failed to scrobble {Track}: {Status}",
                    audioItem.Name,
                    response.StatusCode);
            }
        }
        catch (Exception ex)
        {
            _logger.LogError(ex, "Error sending scrobble for {Track}", audioItem.Name);
        }
    }

    private async Task SendSkipAsync(
        Configuration.PluginConfiguration config,
        Audio audioItem,
        string trackMbid,
        long positionTicks,
        long durationTicks,
        string deviceId,
        User user)
    {
        var artistName = audioItem.Artists?.FirstOrDefault();
        var payload = new
        {
            track_mbid = trackMbid,
            artist_name = artistName,
            track_name = audioItem.Name,
            album_name = audioItem.Album,
            skipped_at = DateTime.UtcNow.ToString("o"),
            played_ms = positionTicks / TimeSpan.TicksPerMillisecond,
            track_duration_ms = durationTicks / TimeSpan.TicksPerMillisecond,
            device_id = deviceId,
            jellyfin_item_id = audioItem.Id.ToString("N"),
            user_name = user.Username
        };

        try
        {
            var request = new HttpRequestMessage(HttpMethod.Post, $"{config.BragiApiUrl.TrimEnd('/')}/api/webhooks/bragi/skip");
            request.Headers.Authorization = new AuthenticationHeaderValue("Bearer", config.BragiApiToken);
            request.Content = new StringContent(
                JsonSerializer.Serialize(payload),
                Encoding.UTF8,
                "application/json");

            var response = await _httpClient.SendAsync(request);

            if (response.IsSuccessStatusCode)
            {
                _logger.LogInformation("Skip recorded: {Artist} - {Track} (played {Ms}ms, user: {User})",
                    artistName ?? "Unknown",
                    audioItem.Name,
                    positionTicks / TimeSpan.TicksPerMillisecond,
                    user.Username);
            }
            else
            {
                _logger.LogWarning("Failed to record skip for {Track}: {Status}",
                    audioItem.Name,
                    response.StatusCode);
            }
        }
        catch (Exception ex)
        {
            _logger.LogError(ex, "Error sending skip for {Track}", audioItem.Name);
        }
    }
}
