#include <cstdint>
#include <vector>

namespace render {

/*
 * The canvas is a grid of cells, each one holding a single character and its style. It is drawn
 * from top to bottom and from left to right, and every cell that did not change since the previous
 * frame is skipped, so that a frame costs as little as possible on slow terminals. The cells are
 * stored row by row in a single vector, which keeps the rows close to each other in memory and
 * makes clearing the whole canvas a single call. A cell whose character is wider than one column
 * is followed by a placeholder cell, which is never drawn but keeps the columns aligned. Resizing
 * the canvas would split those wide characters in two halves: it is disabled until it keeps them.
 */
class Canvas {
public:
    Canvas(std::size_t width, std::size_t height)
        : width_(width), height_(height), cells_(width * height) {}

    std::size_t width() const { return width_; }
    std::size_t height() const { return height_; }

    void clear() {
        for (auto &cell : cells_) {
            cell = Cell{};
        }
    }

    /*
    void resize(std::size_t width, std::size_t height) {
        cells_.resize(width * height);
        width_ = width;
        height_ = height;
    }
    */

private:
    struct Cell {
        char32_t character = U' ';
        std::uint8_t style = 0;
    };

    std::size_t width_;
    std::size_t height_;
    std::vector<Cell> cells_;
};

}  // namespace render
