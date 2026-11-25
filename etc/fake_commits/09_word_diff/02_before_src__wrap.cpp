#include <string_view>
#include <vector>

std::vector<std::string_view> wrap(std::string_view line, std::size_t width) {
    std::vector<std::string_view> rows;
    while (line.size() > width) {
        rows.push_back(line.substr(0, width));
        line.remove_prefix(width);
    }
    rows.push_back(line);
    return rows;
}
