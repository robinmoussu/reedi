#include <string_view>
#include <vector>

std::vector<std::string_view> wrap(std::string_view line, std::size_t width) {
    std::vector<std::string_view> rows;
    while (line.size() > width) {
        auto const end = last_space_before(line, width);
        rows.push_back(line.substr(0, end));
        line.remove_prefix(end);
    }
    rows.push_back(line);
    return rows;
}
