using System.Globalization;

namespace GpuObserve.Monitoring.Parsing;

public static class CsvLine
{
    public static List<string> Split(string line)
    {
        var values = new List<string>();
        if (string.IsNullOrEmpty(line))
        {
            return values;
        }

        var current = new System.Text.StringBuilder();
        var inQuotes = false;

        for (var i = 0; i < line.Length; i++)
        {
            var c = line[i];
            if (c == '"')
            {
                if (inQuotes && i + 1 < line.Length && line[i + 1] == '"')
                {
                    current.Append('"');
                    i++;
                }
                else
                {
                    inQuotes = !inQuotes;
                }
            }
            else if (c == ',' && !inQuotes)
            {
                values.Add(current.ToString().Trim());
                current.Clear();
            }
            else
            {
                current.Append(c);
            }
        }

        values.Add(current.ToString().Trim());
        return values;
    }
}

internal static class ParseUtil
{
    public static double? Double(string? value)
    {
        if (string.IsNullOrWhiteSpace(value))
        {
            return null;
        }

        var text = value.Trim().Trim('[', ']');
        if (text.Equals("N/A", StringComparison.OrdinalIgnoreCase)
            || text.Equals("Not Supported", StringComparison.OrdinalIgnoreCase)
            || text.Equals("-", StringComparison.Ordinal))
        {
            return null;
        }

        return double.TryParse(text, NumberStyles.Float, CultureInfo.InvariantCulture, out var result)
            ? result
            : null;
    }

    public static long? Long(string? value)
    {
        if (string.IsNullOrWhiteSpace(value))
        {
            return null;
        }

        var text = value.Trim().Trim('[', ']');
        if (text.Equals("N/A", StringComparison.OrdinalIgnoreCase)
            || text.Equals("Not Supported", StringComparison.OrdinalIgnoreCase)
            || text.Equals("-", StringComparison.Ordinal))
        {
            return null;
        }

        if (long.TryParse(text, NumberStyles.Integer, CultureInfo.InvariantCulture, out var result))
        {
            return result;
        }

        if (double.TryParse(text, NumberStyles.Float, CultureInfo.InvariantCulture, out var asDouble))
        {
            return (long)Math.Round(asDouble);
        }

        return null;
    }

    public static int? Int(string? value)
    {
        var parsed = Long(value);
        return parsed is >= int.MinValue and <= int.MaxValue ? (int)parsed.Value : null;
    }
}
