using System.Collections.Generic;
using System.Web.Script.Serialization;

namespace CuaSessionHost
{
    internal static class Json
    {
        private static readonly JavaScriptSerializer Ser =
            new JavaScriptSerializer { MaxJsonLength = 32 * 1024 * 1024 };

        public static string Serialize(object value)
        {
            return Ser.Serialize(value);
        }

        public static Dictionary<string, object> Parse(string text)
        {
            if (string.IsNullOrWhiteSpace(text)) return new Dictionary<string, object>();
            return Ser.Deserialize<Dictionary<string, object>>(text)
                   ?? new Dictionary<string, object>();
        }

        public static string Str(Dictionary<string, object> d, string key)
        {
            object v;
            return d.TryGetValue(key, out v) && v != null ? v.ToString() : null;
        }

        public static bool Bool(Dictionary<string, object> d, string key, bool fallback)
        {
            object v;
            if (!d.TryGetValue(key, out v) || v == null) return fallback;
            if (v is bool) return (bool)v;
            bool b;
            return bool.TryParse(v.ToString(), out b) ? b : fallback;
        }

        public static int Int(Dictionary<string, object> d, string key, int fallback)
        {
            object v;
            if (!d.TryGetValue(key, out v) || v == null) return fallback;
            int i;
            return int.TryParse(v.ToString(), out i) ? i : fallback;
        }
    }
}
