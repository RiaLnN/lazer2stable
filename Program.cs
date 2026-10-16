using System.Text;
using System.Text.Json;
using Realms;

Console.OutputEncoding = new UTF8Encoding(false);

if (args.Length < 1)
{
    Console.Error.WriteLine("usage: realm-bridge dump-beatmaps --realm <client.realm>");
    return 2;
}

string? realmPath = null;
for (int i = 1; i < args.Length - 1; i++)
{
    if (args[i] == "--realm") realmPath = args[i + 1];
}
switch (args[0])
{
    case "dump-beatmaps":
        if (realmPath is null || !File.Exists(realmPath))
        {
            Console.Error.WriteLine("error: --realm <path> is required and must exist");
            return 2;
        }
        return DumpBeatmaps(realmPath);

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
            IsDynamic = true,   // схему берём из самого файла
            IsReadOnly = true,  // ничего не пишем в базу lazer
        };

        using var realm = Realm.GetInstance(config);

        var result = new List<object>();
        var seen = new HashSet<int>();

        foreach (var set in realm.DynamicApi.All("BeatmapSet"))
        {
            // Карты, помеченные на удаление, и локальные (OnlineID <= 0) не переносим.
            if (Safe(() => set.DynamicApi.Get<bool>("DeletePending"), false)) continue;

            int onlineId = Safe(() => set.DynamicApi.Get<int>("OnlineID"), -1);
            if (onlineId <= 0 || !seen.Add(onlineId)) continue;

            string title = "", artist = "";
            try
            {
                var beatmaps = set.DynamicApi.GetList<IRealmObject>("Beatmaps");
                if (beatmaps.Count > 0)
                {
                    var meta = beatmaps[0].DynamicApi.Get<IRealmObject>("Metadata");
                    title = meta.DynamicApi.Get<string>("Title") ?? "";
                    artist = meta.DynamicApi.Get<string>("Artist") ?? "";
                }
            }
            catch
            {
                // метаданные нужны только для красивого лога
            }

            result.Add(new { online_id = onlineId, title, artist });
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

static T Safe<T>(Func<T> f, T fallback)
{
    try { return f(); } catch { return fallback; }
}
