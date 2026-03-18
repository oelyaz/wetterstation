# app/Dockerfile
FROM python:3.13-slim

WORKDIR /app

# Install dependencies
COPY requirements.txt .
RUN pip install --no-cache-dir -r requirements.txt

# Copy the script
COPY db_subscriber/db_subscriber.py .

# Run the script
CMD ["python", "db_subscriber.py"]