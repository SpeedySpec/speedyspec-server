/*
 * A bounded, offline probe over libavformat. The file arrives as a descriptor and is read through a custom AVIO
 * context, so FFmpeg never opens anything itself: no protocols are built, nested opens are refused, and reads stop at
 * a byte budget. An interrupt callback enforces a deadline. Everything found is copied into plain structs (see
 * shim.h) before the FFmpeg context is closed.
 */
#define _XOPEN_SOURCE 700
#include "shim.h"

#include <errno.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <unistd.h>

#include <libavcodec/avcodec.h>
#include <libavformat/avformat.h>
#include <libavutil/channel_layout.h>
#include <libavutil/dict.h>
#include <libavutil/log.h>
#include <libavutil/mem.h>
#include <libavutil/pixdesc.h>
#include <libavutil/samplefmt.h>
#include <libavutil/time.h>

#define SS_IO_BUFFER 65536

typedef struct {
    int fd;
    int64_t size;
    int64_t pos;
    int64_t read_total;
    int64_t max_bytes;
    int64_t deadline_us;
    int over_budget;
    int timed_out;
} ss_io;

static int ss_read(void *opaque, uint8_t *buf, int buf_size) {
    ss_io *io = opaque;
    if (io->read_total >= io->max_bytes) {
        io->over_budget = 1;
        return AVERROR_EXIT;
    }
    int64_t room = io->max_bytes - io->read_total;
    if (buf_size > room) buf_size = (int)room;
    ssize_t n;
    do {
        n = pread(io->fd, buf, (size_t)buf_size, io->pos);
    } while (n < 0 && errno == EINTR);
    if (n < 0) return AVERROR(errno);
    if (n == 0) return AVERROR_EOF;
    io->pos += n;
    io->read_total += n;
    return (int)n;
}

static int64_t ss_seek(void *opaque, int64_t offset, int whence) {
    ss_io *io = opaque;
    int64_t target;
    switch (whence & ~AVSEEK_FORCE) {
    case AVSEEK_SIZE:
        return io->size;
    case SEEK_SET:
        target = offset;
        break;
    case SEEK_CUR:
        target = io->pos + offset;
        break;
    case SEEK_END:
        target = io->size + offset;
        break;
    default:
        return AVERROR(EINVAL);
    }
    if (target < 0) return AVERROR(EINVAL);
    io->pos = target;
    return target;
}

static int ss_interrupt(void *opaque) {
    ss_io *io = opaque;
    if (av_gettime_relative() > io->deadline_us) {
        io->timed_out = 1;
        return 1;
    }
    return 0;
}

/* Nested opens (playlists, references, sidecars) are refused outright. */
static int ss_io_open(struct AVFormatContext *s, AVIOContext **pb, const char *url, int flags, AVDictionary **options) {
    (void)s;
    (void)pb;
    (void)url;
    (void)flags;
    (void)options;
    return AVERROR(EPERM);
}

static char *ss_strdup(const char *s) {
    return s ? strdup(s) : NULL;
}

/* Copies a dictionary, dropping values longer than max_bytes. Returns -1 on allocation failure. */
static int ss_copy_tags(const AVDictionary *dict, int64_t max_bytes, ss_tag **out, int32_t *count) {
    *out = NULL;
    *count = 0;
    int n = av_dict_count(dict);
    if (n <= 0) return 0;
    ss_tag *tags = calloc((size_t)n, sizeof(ss_tag));
    if (!tags) return -1;
    const AVDictionaryEntry *e = NULL;
    int i = 0;
    while ((e = av_dict_iterate(dict, e)) && i < n) {
        if ((int64_t)strlen(e->value) > max_bytes) continue;
        tags[i].key = strdup(e->key);
        tags[i].value = strdup(e->value);
        if (!tags[i].key || !tags[i].value) {
            free(tags[i].key);
            free(tags[i].value);
            *out = tags;
            *count = i;
            return -1;
        }
        i++;
    }
    *out = tags;
    *count = i;
    return 0;
}

static void ss_free_tags(ss_tag *tags, int32_t count) {
    for (int32_t i = 0; i < count; i++) {
        free(tags[i].key);
        free(tags[i].value);
    }
    free(tags);
}

void ss_global_init(int64_t max_alloc) {
    if (max_alloc > 0) av_max_alloc((size_t)max_alloc);
    av_log_set_level(AV_LOG_QUIET);
}

void ss_free(ss_result *r) {
    if (!r) return;
    free(r->format_name);
    ss_free_tags(r->tags, r->tag_count);
    for (int32_t i = 0; i < r->stream_count; i++) {
        ss_stream *s = &r->streams[i];
        free(s->codec_name);
        free(s->profile);
        free(s->channel_layout);
        free(s->pix_fmt);
        free(s->sample_fmt);
        free(s->picture);
        ss_free_tags(s->tags, s->tag_count);
    }
    free(r->streams);
    free(r);
}

static int ss_media_type(enum AVMediaType t) {
    switch (t) {
    case AVMEDIA_TYPE_VIDEO: return 0;
    case AVMEDIA_TYPE_AUDIO: return 1;
    case AVMEDIA_TYPE_DATA: return 2;
    case AVMEDIA_TYPE_SUBTITLE: return 3;
    case AVMEDIA_TYPE_ATTACHMENT: return 4;
    default: return -1;
    }
}

static int ss_copy_stream(const AVStream *st, const ss_limits *limits, ss_stream *s) {
    const AVCodecParameters *par = st->codecpar;
    s->media_type = ss_media_type(par->codec_type);
    s->codec_name = ss_strdup(avcodec_get_name(par->codec_id));
    s->profile = ss_strdup(avcodec_profile_name(par->codec_id, par->profile));
    s->width = par->width;
    s->height = par->height;
    s->sample_rate = par->sample_rate;
    s->channels = par->ch_layout.nb_channels;
    if (par->codec_type == AVMEDIA_TYPE_AUDIO && par->ch_layout.nb_channels > 0) {
        char layout[128];
        if (av_channel_layout_describe(&par->ch_layout, layout, sizeof layout) > 0) s->channel_layout = strdup(layout);
    }
    s->bits_per_raw_sample = par->bits_per_raw_sample;
    s->bits_per_coded_sample = par->bits_per_coded_sample;
    s->bit_rate = par->bit_rate;
    AVRational fr = st->avg_frame_rate.num > 0 ? st->avg_frame_rate : st->r_frame_rate;
    s->fr_num = fr.num;
    s->fr_den = fr.den;
    if (par->codec_type == AVMEDIA_TYPE_VIDEO && par->format >= 0) {
        s->pix_fmt = ss_strdup(av_get_pix_fmt_name((enum AVPixelFormat)par->format));
    }
    if (par->codec_type == AVMEDIA_TYPE_AUDIO && par->format >= 0) {
        s->sample_fmt = ss_strdup(av_get_sample_fmt_name((enum AVSampleFormat)par->format));
    }
    s->attached_pic = (st->disposition & AV_DISPOSITION_ATTACHED_PIC) ? 1 : 0;
    if (s->attached_pic && st->attached_pic.data && st->attached_pic.size > 0 &&
        st->attached_pic.size <= limits->max_picture_bytes) {
        s->picture = malloc((size_t)st->attached_pic.size);
        if (!s->picture) return -1;
        memcpy(s->picture, st->attached_pic.data, (size_t)st->attached_pic.size);
        s->picture_size = st->attached_pic.size;
    }
    return ss_copy_tags(st->metadata, limits->max_tag_bytes, &s->tags, &s->tag_count);
}

static int32_t ss_status(const ss_io *io, int32_t otherwise) {
    if (io->timed_out) return SS_TIMEOUT;
    if (io->over_budget) return SS_LIMIT_BYTES;
    return otherwise;
}

int32_t ss_probe(int fd, const ss_limits *limits, ss_result **out) {
    *out = NULL;
    struct stat st;
    if (fstat(fd, &st) != 0 || !S_ISREG(st.st_mode)) return SS_IO;

    ss_io io = {
        .fd = fd,
        .size = (int64_t)st.st_size,
        .max_bytes = limits->max_bytes,
        .deadline_us = av_gettime_relative() + limits->deadline_ms * 1000,
    };
    int32_t status = SS_OK;
    AVFormatContext *ctx = NULL;
    AVIOContext *pb = NULL;
    AVDictionary *opts = NULL;
    ss_result *r = NULL;

    uint8_t *buffer = av_malloc(SS_IO_BUFFER);
    if (!buffer) return SS_NOMEM;
    pb = avio_alloc_context(buffer, SS_IO_BUFFER, 0, &io, ss_read, NULL, ss_seek);
    if (!pb) {
        av_free(buffer);
        return SS_NOMEM;
    }
    ctx = avformat_alloc_context();
    if (!ctx) {
        status = SS_NOMEM;
        goto done;
    }
    ctx->pb = pb;
    ctx->flags |= AVFMT_FLAG_CUSTOM_IO;
    ctx->interrupt_callback.callback = ss_interrupt;
    ctx->interrupt_callback.opaque = &io;
    ctx->io_open = ss_io_open;
    ctx->max_streams = limits->max_streams + 1;
    int64_t probesize = limits->max_bytes < (5 << 20) ? limits->max_bytes : (5 << 20);
    if (probesize < 32) probesize = 32;
    ctx->probesize = probesize;
    ctx->max_analyze_duration = 5 * AV_TIME_BASE;
    av_dict_set(&opts, "protocol_whitelist", "none", 0);
    av_dict_set(&opts, "enable_drefs", "0", 0);
    av_dict_set(&opts, "use_absolute_path", "0", 0);

    const AVInputFormat *fmt = NULL;
    int ret = av_probe_input_buffer2(pb, &fmt, "", ctx, 0, (unsigned)probesize);
    if (ret < 0 || !fmt) {
        status = ss_status(&io, ret == AVERROR(ENOMEM) ? SS_NOMEM : SS_UNRECOGNIZED);
        goto done;
    }
    ret = avformat_open_input(&ctx, NULL, fmt, &opts);
    if (ret < 0) {
        /* avformat_open_input frees ctx on failure. */
        ctx = NULL;
        status = ss_status(&io, ret == AVERROR(ENOMEM) ? SS_NOMEM : SS_TRUNCATED);
        goto done;
    }
    ret = avformat_find_stream_info(ctx, NULL);
    if (io.timed_out || io.over_budget) {
        status = ss_status(&io, SS_OK);
        goto done;
    }
    if (ret < 0 && ctx->nb_streams == 0) {
        status = ss_status(&io, ret == AVERROR(ENOMEM) ? SS_NOMEM : SS_TRUNCATED);
        goto done;
    }
    if ((int64_t)ctx->nb_streams > limits->max_streams) {
        status = SS_LIMIT_STREAMS;
        goto done;
    }

    r = calloc(1, sizeof(ss_result));
    if (!r) {
        status = SS_NOMEM;
        goto done;
    }
    r->format_name = ss_strdup(ctx->iformat->name);
    r->duration_us = ctx->duration == AV_NOPTS_VALUE ? INT64_MIN : ctx->duration;
    r->bit_rate = ctx->bit_rate;
    if (ss_copy_tags(ctx->metadata, limits->max_tag_bytes, &r->tags, &r->tag_count) != 0) {
        status = SS_NOMEM;
        goto done;
    }
    if (ctx->nb_streams > 0) {
        r->streams = calloc(ctx->nb_streams, sizeof(ss_stream));
        if (!r->streams) {
            status = SS_NOMEM;
            goto done;
        }
        for (unsigned i = 0; i < ctx->nb_streams; i++) {
            r->stream_count = (int32_t)i + 1;
            if (ss_copy_stream(ctx->streams[i], limits, &r->streams[i]) != 0) {
                status = SS_NOMEM;
                goto done;
            }
        }
    }
    r->bytes_read = io.read_total;

done:
    av_dict_free(&opts);
    if (ctx) avformat_close_input(&ctx);
    if (pb) {
        av_freep(&pb->buffer);
        avio_context_free(&pb);
    }
    if (status == SS_OK) {
        *out = r;
    } else {
        ss_free(r);
    }
    return status;
}
