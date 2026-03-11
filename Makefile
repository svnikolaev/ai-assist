# Makefile для ai-assist

# Имя символической ссылки (по умолчанию 'assist', чтобы избежать конфликта с ai-shell)
# Если вы хотите использовать 'ai', убедитесь, что нет другого приложения с таким же именем.
LINK_NAME ?= ask

.PHONY: all test build install link clean uninstall

# Цель по умолчанию: тест, сборка, установка и создание ссылки
all: test build install link

# Запуск тестов (если они есть)
test:
	cargo test

# Сборка релизной версии
build:
	cargo build --release

# Установка через cargo install из текущего пути
install:
	cargo install --path .

# Создание символической ссылки с именем $(LINK_NAME) в ~/.cargo/bin
link:
	@if [ -f ~/.cargo/bin/ai-assist ]; then \
		ln -sf ~/.cargo/bin/ai-assist ~/.cargo/bin/$(LINK_NAME); \
		echo "Ссылка создана: ~/.cargo/bin/$(LINK_NAME) -> ~/.cargo/bin/ai-assist"; \
	else \
		echo "Бинарник ai-assist не найден в ~/.cargo/bin. Сначала выполните make install."; \
		exit 1; \
	fi

# Очистка временных файлов сборки
clean:
	cargo clean

# Удаление установленного бинарника и ссылки
uninstall:
	@rm -f ~/.cargo/bin/ai-assist ~/.cargo/bin/$(LINK_NAME)
	@echo "Удалены ai-assist и $(LINK_NAME) из ~/.cargo/bin (если существовали)."