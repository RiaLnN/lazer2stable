using System.Text;
using System.Text.Json;
using Realms;

Console.OutputEncoding = new UTF8Encoding(false);

if (args.Length < 1)
{
    Console.Error.WriteLine("usage: realm-bridge <dump-beatmaps|dump-skins> --realm <client.realm>");
    return 2;
}

string? realmPath = null;
for (int i = 1; i < args.Length - 1; i++)
{
    if (args[i] == "--realm") realmPath = args[i + 1];
}

if (realmPath is null || !File.Exists(realmPath))
{
    Console.Error.WriteLine("error: --realm <path> is required and file must exist");
    return 2;
}

switch (args[0])
{
    case "dump-beatmaps":
        return DumpBeatmaps(realmPath);

    case "dump-skins":
        return DumpSkins(realmPath);

    default:
        Console.Error.WriteLine($"unknown command: {args[0]}");
        return 2;
}

static int DumpBeatmaps(string realmPath)
{
    try
    {
        var config = new RealmConfiguration(realmPath)
        {
            IsDynamic = true,   // Read schema dynamically from the database
            IsReadOnly = true,  // Strictly read-only
        };

        using var realm = Realm.GetInstance(config);

        var result = new List<object>();
        var seen = new HashSet<int>();

        foreach (var set in realm.DynamicApi.All("BeatmapSet"))
        {
            if (Safe(() => set.DynamicApi.Get<bool>("DeletePending"), false)) continue;

            int onlineId = Safe(() => set.DynamicApi.Get<int>("OnlineID"), -1);
            if (onlineId <= 0 || !seen.Add(onlineId)) continue;

            string title = "", artist = "";
            try
            {
                var beatmaps = set.DynamicApi.GetList<IRealmObjectBase>("Beatmaps");
                if (beatmaps.Count > 0)
                {
                    var meta = beatmaps[0].DynamicApi.Get<IRealmObjectBase>("Metadata");
                    title = meta.DynamicApi.Get<string>("Title") ?? "";
                    artist = meta.DynamicApi.Get<string>("Artist") ?? "";
                }
            }
            catch
            {
                // Metadata is optional for display
            }

            var files = ExtractFiles(set);

            result.Add(new { online_id = onlineId, title, artist, files });
        }

        Console.Error.WriteLine($"found {result.Count} online beatmap sets");
        Console.Out.WriteLine(JsonSerializer.Serialize(result));
        return 0;
    }
    catch (Exception ex)
    {
        Console.Error.WriteLine($"error: {ex.GetType().Name}: {ex.Message}");
        return 1;
    }
}

static int DumpSkins(string realmPath)
{
    try
    {
        var config = new RealmConfiguration(realmPath)
        {
            IsDynamic = true,
            IsReadOnly = true,
        };

        using var realm = Realm.GetInstance(config);

        var result = new List<object>();

        foreach (var skin in realm.DynamicApi.All("Skin"))
        {
            if (Safe(() => skin.DynamicApi.Get<bool>("DeletePending"), false)) continue;
            bool isProtected = Safe(() => skin.DynamicApi.Get<bool>("Protected"), false);
            if (isProtected) continue;

            string name = Safe(() => skin.DynamicApi.Get<string>("Name"), "") ?? "";
            if (string.IsNullOrWhiteSpace(name)) continue;

            var files = ExtractFiles(skin);
            if (files.Count == 0) continue;

            string id = Safe(() => skin.DynamicApi.Get<object>("ID")?.ToString(), Guid.NewGuid().ToString()) ?? "";
            string creator = Safe(() => skin.DynamicApi.Get<string>("Creator"), "") ?? "";

            result.Add(new { id, name, creator, files });
        }

        Console.Error.WriteLine($"found {result.Count} custom skins");
        Console.Out.WriteLine(JsonSerializer.Serialize(result));
        return 0;
    }
    catch (Exception ex)
    {
        Console.Error.WriteLine($"error: {ex.GetType().Name}: {ex.Message}");
        return 1;
    }
}

static List<object> ExtractFiles(IRealmObjectBase parent)
{
    var list = new List<object>();
    try
    {
        var filesList = parent.DynamicApi.GetList<IRealmObjectBase>("Files");
        foreach (var fileUsage in filesList)
        {
            string filename = Safe(() => fileUsage.DynamicApi.Get<string>("Filename"), "") ?? "";
            var fileObj = Safe(() => fileUsage.DynamicApi.Get<IRealmObjectBase>("File"), null);
            string hash = fileObj != null ? Safe(() => fileObj.DynamicApi.Get<string>("Hash"), "") ?? "" : "";

            if (!string.IsNullOrEmpty(filename) && !string.IsNullOrEmpty(hash))
            {
                list.Add(new { filename, hash });
            }
        }
    }
    catch (Exception ex)
    {
        Console.Error.WriteLine("ExtractFiles error: " + ex.GetType().Name + ": " + ex.Message);
    }
    return list;
}

static T Safe<T>(Func<T> f, T fallback)
{
    try { return f(); } catch { return fallback; }
}
