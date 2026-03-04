# Myriad Docker Deployment Makefile
# Provides simplified commands to manage Docker deployment

.PHONY: help deploy start stop restart logs status clean build ps backend-test

# Default target
.DEFAULT_GOAL := help

# Help information
help:
	@echo "Myriad Docker Deployment Commands"
	@echo ""
	@echo "Usage: make [command]"
	@echo ""
	@echo "Available commands:"
	@echo "  deploy     - One-click deployment (first time)"
	@echo "  start      - Start all services"
	@echo "  stop       - Stop all services"
	@echo "  restart    - Restart all services"
	@echo "  logs       - View real-time logs"
	@echo "  status     - View service status"
	@echo "  ps         - View container list"
	@echo "  build      - Rebuild images"
	@echo "  clean      - Clean all resources"
	@echo ""

# One-click deployment
deploy:
	@echo "Starting Myriad deployment..."
	@if [ ! -f .env ]; then \
		echo "Creating .env file..."; \
		cp .env.docker .env; \
		echo ""; \
		echo "⚠️  Please edit .env file first, modify passwords and keys"; \
		echo ""; \
		read -p "Press Enter to continue editing..." dummy; \
		$${EDITOR:-nano} .env; \
	fi
	@docker-compose up -d --build
	@echo ""
	@echo "✓ Deployment complete!"
	@echo ""
	@echo "Access URLs:"
	@echo "  Frontend: http://localhost:4321"
	@echo "  Backend:  http://localhost:3000"

# Start services
start:
	docker-compose up -d
	@echo "✓ Services started"

# Stop services
stop:
	docker-compose down
	@echo "✓ Services stopped"

# Restart services
restart:
	docker-compose restart
	@echo "✓ Services restarted"

# View logs
logs:
	docker-compose logs -f

# View status
status:
	@docker-compose ps
	@echo ""
	@echo "Health status:"
	@docker inspect myriad-postgres --format='PostgreSQL: {{.State.Health.Status}}' 2>/dev/null || true
	@docker inspect myriad-backend --format='Backend: {{.State.Health.Status}}' 2>/dev/null || true
	@docker inspect myriad-frontend --format='Frontend: {{.State.Health.Status}}' 2>/dev/null || true

# View container list
ps:
	docker-compose ps

# Rebuild
build:
	docker-compose build --no-cache
	docker-compose up -d
	@echo "✓ Rebuild complete"

# Clean resources
clean:
	@echo "⚠️  This will delete all containers, images and volumes"
	@read -p "Confirm to continue? (yes/N): " confirm; \
	if [ "$$confirm" = "yes" ]; then \
		docker-compose down -v --rmi all; \
		echo "✓ Cleanup complete"; \
	else \
		echo "Cancelled"; \
	fi

# Database backup
backup:
	@mkdir -p backups
	@docker exec myriad-postgres pg_dump -U myriad myriad > backups/backup_$$(date +%Y%m%d_%H%M%S).sql
	@echo "✓ Backup complete: backups/backup_$$(date +%Y%m%d_%H%M%S).sql"

# Enter backend container
shell-backend:
	docker exec -it myriad-backend sh

# Enter database
shell-db:
	docker exec -it myriad-postgres psql -U myriad -d myriad

# Enter frontend container
shell-frontend:
	docker exec -it myriad-frontend sh

backend-test:
	@cd backend && cargo test --test $(TEST) -- $(ARGS)
