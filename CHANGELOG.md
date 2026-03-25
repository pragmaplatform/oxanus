# Changelog

All notable changes to this project will be documented in this file.

## [0.9.0]

### Added
- Throttling: support custom cost per job
- Throttling: skip Redis calls when throttle cost is zero
- Store error message on dead and retried jobs
- `JobMeta`: don't serialize None fields

### Changed
- Relax sentry version requirement
- Update dependencies

## [0.8.20]

### Added
- Conductor workspace setup and archive scripts

## [0.8.19]

### Added
- Failed count to global stats

### Changed
- Tweak dead trimming

## [0.8.18]

### Fixed
- Fix resurrect orphaning (with regression test)

## [0.8.17]

### Added
- Track `started_at` for a job

## [0.8.16]

### Added
- Expose `delete_job`
- Error when updating state of non-existing job

## [0.8.15]

### Added
- Add resurrect flag

## [0.8.14]

### Changed
- Rename catalog function

## [0.8.13]

### Added
- Include queues in catalog

## [0.8.12]

### Added
- Workers catalog

## [0.8.11]

### Added
- `list_scheduled` function

## [0.8.10]

### Fixed
- Fix `list_dead` and `list_retries`

## [0.8.9]

### Added
- `list_dead` and `list_retries` functions

## [0.8.8]

### Added
- Export `JobEnvelope`

## [0.8.7]

### Added
- `list_queue_jobs` and `wipe_queue` functions

## [0.8.6]

### Added
- Export stats types

## [0.8.4]

### Added
- `JobMeta`: add `created_at`/`scheduled_at` functions returning datetime

### Changed
- Revert back to deadpool-redis 0.22

## [0.8.2]

### Added
- Export `JobState` and `JobMeta`

### Changed
- Update dependencies

## [0.8.1]

### Added
- Prometheus metrics

### Fixed
- Fix reported latency

## [0.8.0]

### Added
- Worker derive macro
- Queue derive macro
- Component registry with macros
- Allow unique id to be `None`
- Allow register cron worker with schedule override
- `has_registered_cron_worker` function
- `max_retries` support custom function

### Changed
- Change Cron worker API and update examples
- Namespace `unique_id` with job name
- Restore `register_cron_worker` API
- Update dependencies

## [0.7.0]

### Added
- Report process' `started_at`

## [0.6.3]

### Changed
- Update dependencies

## [0.6.2]

### Changed
- Upgrade dependencies (Redis to 1.0)

## [0.6.0]

### Changed
- Rework storage builder

## [0.5.2]

### Added
- Implement on-conflict strategy for unique jobs

## [0.5.1]

### Changed
- Enqueue immediately with `enqueue_at` when time is lower than now

## [0.5.0]

### Added
- Expose `enqueue_at` in public interface

### Fixed
- Fix cron/unique jobs not being retried

## [0.4.0]

### Added
- Optimize connection management
- Optimize `enqueue_scheduled` with pipeline

### Changed
- Switch time types from `u64` to `i64`
- Improve tracing
- Change age to latency and add `scheduled_at`

### Fixed
- Fix cleanup timestamp check

## [0.3.x]

### Added
- Implement shutdown timeout
- Add cron job validation
- Firehose (optional, disabled by default)
- Tracing instrument feature
- Multi-crate workspace structure (`oxanus` + `oxanus-api`)
- Latency max to global stats
- Global stats enhancements
- Process reporting and stats expansion
- Basic instrumentation
- Resumable jobs via job state
- `from_env_var` for storage builder
- `latency_ms` to storage
- Record basic global stats for all queues
- Add drain

### Changed
- Improve error handling
- Improve storage builder
- Reliability improvements
- Rework stats with grouping of dynamic queues

### Fixed
- Fix handling of critical failure
- Fix double in dynamic queue key
- Account for job not existing anymore when retrying
- Fix performance regression
- Fix key prefixes
- Fix windows compatibility
- Fix macos compatibility

## [0.2.0]

### Added
- Support for cron jobs
- Namespace storage support
- Redis pool (deadpool-redis)
- Dynamic queues support
- Sentry integration (minimal)
- Graceful panic handling
- Dead queue trimming

### Changed
- Rename `WorkerContext` to `Context`
- Clean up public API
- Accept any future as shutdown signal

## [0.1.0]

### Added
- Initial release
- Job processing with Redis backend
- Scheduling and retrying
- Throttling
- Unique jobs
- Job expiration
- Resilient jobs
- Graceful shutdown
- Dynamic queues
- Concurrency control per queue
