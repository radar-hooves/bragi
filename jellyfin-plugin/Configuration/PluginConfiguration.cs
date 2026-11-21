using MediaBrowser.Model.Plugins;

namespace Jellyfin.Plugin.BragiScrobbler.Configuration;

/// <summary>
/// Plugin configuration.
/// </summary>
public class PluginConfiguration : BasePluginConfiguration
{
    /// <summary>
    /// Gets or sets the Bragi API URL.
    /// </summary>
    public string BragiApiUrl { get; set; } = "http://localhost:8000";

    /// <summary>
    /// Gets or sets the Bragi API token.
    /// </summary>
    public string BragiApiToken { get; set; } = string.Empty;

    /// <summary>
    /// Gets or sets a value indicating whether scrobbling is enabled.
    /// </summary>
    public bool EnableScrobbling { get; set; } = true;

    /// <summary>
    /// Gets or sets a value indicating whether skip tracking is enabled.
    /// </summary>
    public bool EnableSkipTracking { get; set; } = true;
}
